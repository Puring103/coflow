//! 创建线程拥有全部句柄。表借用只覆盖查找和移除，不跨 Host 回调或析构。
use super::Entry;
use std::{cell::{Cell, RefCell}, collections::BTreeMap, sync::atomic::{AtomicU64, Ordering}};
pub(super) type ThreadBound<T> = std::rc::Rc<T>;
static NEXT_HANDLE: AtomicU64 = AtomicU64::new(1);

#[derive(Default)]
struct LocalEntries(BTreeMap<u64, Entry>, BTreeMap<u64, u64>);
thread_local! {
    static SHUTTING_DOWN: Cell<bool> = const { Cell::new(false) };
    static LOCAL_ENTRIES: RefCell<LocalEntries> = RefCell::new(LocalEntries::default());
}
pub(super) fn insert(entry: Entry) -> Result<u64, String> {
    if SHUTTING_DOWN.try_with(Cell::get).unwrap_or(true) { return Err("creating thread is shutting down".into()); }
    // 全局仅生成不复用身份，所有实际数据和生命周期都归创建线程。
    let id = NEXT_HANDLE.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
        .map_err(|_| "handle identity exhausted")?;
    LOCAL_ENTRIES.try_with(|entries| {
        let mut entries = entries.borrow_mut();
        if let Entry::Runtime(runtime) = &entry { entries.1.insert(runtime.identity(), id); }
        entries.0.insert(id, entry)
    })
        .map_err(|_| "creating thread is shutting down")?;
    Ok(id)
}
pub(super) fn get(id: u64) -> Result<Entry, String> {
    LOCAL_ENTRIES.try_with(|entries| entries.borrow().0.get(&id).cloned())
        .map_err(|_| "creating thread is shutting down")?
        .ok_or_else(|| "invalid handle or access outside its creating thread".into())
}
fn is_busy(entry: &Entry) -> bool {
    match entry {
        Entry::Runtime(runtime) => runtime.is_executing(),
        Entry::Builder(builder) => builder.try_borrow_mut().is_err(),
        #[cfg(feature = "cft-compiler")]
        Entry::Compiler(compiler) => compiler.try_borrow_mut().is_err(),
        _ => false,
    }
}
fn finish(entry: Entry) {
    if let Entry::Runtime(runtime) = &entry { let _ = runtime.release(); }
    drop(entry);
}
pub(super) fn dispose(id: u64) -> Result<(), String> {
    let removed = LOCAL_ENTRIES.try_with(|entries| {
        let mut entries = entries.borrow_mut();
        let entry = entries.0.get(&id).ok_or("invalid handle or disposal outside its creating thread")?;
        if is_busy(entry) { return Err("Runtime busy"); }
        let removed = entries.0.remove(&id).expect("句柄已验证");
        if let Entry::Runtime(runtime) = &removed { entries.1.remove(&runtime.identity()); }
        Ok(removed)
    }).map_err(|_| "creating thread is shutting down")??;
    finish(removed);
    Ok(())
}
pub(super) fn shutdown() -> Result<(), String> {
    if SHUTTING_DOWN.try_with(Cell::get).unwrap_or(true) { return Err("creating thread is shutting down".into()); }
    let entries = LOCAL_ENTRIES.try_with(|entries| {
        let mut entries = entries.borrow_mut();
        if entries.0.values().any(is_busy) {
            return Err("Runtime busy");
        }
        entries.1.clear();
        Ok(std::mem::take(&mut entries.0))
    }).map_err(|_| "creating thread is shutting down")??;
    // 关闭期间 Host 析构可以重入，但不能向已关闭的域登记新资源。
    struct ShutdownGuard;
    impl Drop for ShutdownGuard {
        fn drop(&mut self) { let _ = SHUTTING_DOWN.try_with(|state| state.set(false)); }
    }
    SHUTTING_DOWN.with(|state| state.set(true));
    let _guard = ShutdownGuard;
    for (_, entry) in entries { finish(entry); }
    Ok(())
}
pub(super) fn take_buffer(id: u64) -> Result<Vec<u8>, String> {
    let entry = LOCAL_ENTRIES.try_with(|entries| {
        let mut entries = entries.borrow_mut();
        if !matches!(entries.0.get(&id), Some(Entry::Buffer(_))) { return Err("expected returned Host buffer"); }
        Ok(entries.0.remove(&id).expect("缓冲句柄已验证"))
    }).map_err(|_| "creating thread is shutting down")??;
    let Entry::Buffer(bytes) = entry else { unreachable!() };
    // 正常消费独占缓冲直接转移所有权；已有借用保留时才复制。
    Ok(ThreadBound::unwrap_or_clone(bytes))
}

pub(super) fn runtime_handle(identity: u64) -> Result<u64, String> {
    LOCAL_ENTRIES.try_with(|entries| entries.borrow().1.get(&identity).copied())
        .map_err(|_| "execution thread is shutting down")?
        .ok_or_else(|| "Runtime handle is no longer registered".into())
}
