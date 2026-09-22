//! 仅测试链接分配观测依赖；生产库不替换分配器。统计当前线程的请求字节与峰值。
pub(crate) fn measure<T>(operation: impl FnOnce() -> T) -> (T, allocation_counter::AllocationInfo) {
    let mut result = None;
    let statistics = allocation_counter::measure(|| { result = Some(operation()); });
    (result.expect("测量闭包已运行"), statistics)
}
