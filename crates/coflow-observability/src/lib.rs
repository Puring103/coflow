//! 宿主日志初始化、跨进程容量控制与本地诊断快照。
use fs2::FileExt;
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::PathBuf,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};
static RUN_ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();
pub fn run_id() -> &'static str {
    RUN_ID.get().map_or("uninitialized", String::as_str)
}
pub use tracing;
const LIMIT: u64 = 10 * 1024 * 1024;
const FILES: usize = 5;
static SEQUENCE: AtomicU64 = AtomicU64::new(1);
pub fn identifier() -> String {
    format!(
        "{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}
pub fn directory(application: &str) -> io::Result<PathBuf> {
    dirs::data_local_dir()
        .map(|path| path.join("Coflow").join("logs").join(application))
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "无法确定本机日志目录"))
}
#[derive(Debug, Clone)]
struct RollingWriter {
    directory: PathBuf,
    failures: Arc<AtomicU64>,
}
impl RollingWriter {
    fn write_record(&self, bytes: &[u8]) -> io::Result<()> {
        // 所有进程共用短期文件锁，轮转不会删除另一个进程正在写入的文件。
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.directory.join("writer.lock"))?;
        lock.lock_exclusive()?;
        for index in 0..FILES {
            let path = self.directory.join(format!("events.{index}.jsonl"));
            if fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.elapsed().ok())
                .is_some_and(|age| age.as_secs() > 7 * 86400)
            {
                fs::remove_file(path)?;
            }
        }
        let current = self.directory.join("events.0.jsonl");
        if fs::metadata(&current).map_or(0, |m| m.len()) + bytes.len() as u64 > LIMIT {
            for index in (1..FILES).rev() {
                let destination = self.directory.join(format!("events.{index}.jsonl"));
                if destination.exists() {
                    fs::remove_file(&destination)?;
                }
                let source = self.directory.join(format!("events.{}.jsonl", index - 1));
                if source.exists() {
                    fs::rename(source, destination)?;
                }
            }
        }
        if bytes.len() as u64 > LIMIT {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "日志事件超过容量限制",
            ));
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(current)?
            .write_all(bytes)
    }
}
impl Write for RollingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if let Err(error) = self.write_record(bytes) {
            // 故障计数由诊断界面读取；不能把日志写入失败再次送入日志队列。
            self.failures.fetch_add(1, Ordering::Relaxed);
            return Err(error);
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
pub struct Logging {
    pub run_id: String,
    pub directory: PathBuf,
    failures: Arc<AtomicU64>,
    dropped: tracing_appender::non_blocking::ErrorCounter,
    _guard: tracing_appender::non_blocking::WorkerGuard,
}
impl Logging {
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"run_id": self.run_id, "write_failures": self.failures.load(Ordering::Relaxed), "dropped_events": self.dropped.dropped_lines(), "version": env!("CARGO_PKG_VERSION"), "os": std::env::consts::OS, "arch": std::env::consts::ARCH})
    }
    pub fn snapshot(&self) -> io::Result<serde_json::Value> {
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.directory.join("writer.lock"))?;
        lock.lock_exclusive()?;
        // 反馈包只取当前文件末尾最多 1 MiB；不会收集项目文件或完整环境变量。
        use std::io::{Read, Seek, SeekFrom};
        let mut logs = String::new();
        if let Ok(mut file) = fs::File::open(self.directory.join("events.0.jsonl")) {
            let length = file.metadata()?.len();
            file.seek(SeekFrom::Start(length.saturating_sub(1024 * 1024)))?;
            let mut bytes = Vec::new();
            file.take(1024 * 1024).read_to_end(&mut bytes)?;
            logs = String::from_utf8_lossy(&bytes).into_owned();
            if length > 1024 * 1024 {
                logs = logs
                    .split_once('\n')
                    .map_or(String::new(), |(_, rest)| rest.to_string());
            }
        }
        let panic = fs::read(self.directory.join("last-panic.json"))
            .ok()
            .filter(|bytes| bytes.len() < 16384)
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
        Ok(serde_json::json!({"environment": self.status(), "logs": logs, "last_panic": panic}))
    }
}
pub fn initialize(application: &str) -> io::Result<Logging> {
    let directory = directory(application)?;
    fs::create_dir_all(&directory)?;
    let failures = Arc::new(AtomicU64::new(0));
    let writer = RollingWriter {
        directory: directory.clone(),
        failures: failures.clone(),
    };
    let (writer, guard) = tracing_appender::non_blocking::NonBlockingBuilder::default()
        .buffered_lines_limit(512)
        .lossy(true)
        .finish(writer);
    let dropped = writer.error_counter();
    let layer = tracing_subscriber::fmt::layer()
        .json()
        .with_ansi(false)
        .with_writer(writer)
        .with_filter(tracing_subscriber::filter::filter_fn(|metadata| {
            // 第三方库日志可能携带 URL 或请求正文，默认只收集 Coflow 自身的受控事件。
            *metadata.level() <= tracing::Level::INFO
                && (metadata.target().starts_with("coflow")
                    || metadata.target().starts_with("cfd_editor"))
        }));
    tracing_subscriber::registry()
        .with(layer)
        .try_init()
        .map_err(|error| io::Error::other(error.to_string()))?;
    let run_id = identifier();
    let _ = RUN_ID.set(run_id.clone());
    let panic_run = run_id.clone();
    let panic_directory = directory.clone();
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // panic 内容可能携带业务数据，仅记录代码位置和关联标识。
        tracing::error!(run_id = %panic_run, location = ?info.location(), event = "panic");
        // 紧急记录不依赖异步队列或轮转锁，只保留最后一次 panic 的受限元数据。
        let location = info.location().map(|location| {
            format!(
                "{}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            )
        });
        if let Ok(bytes) = serde_json::to_vec(
            &serde_json::json!({"event": "panic", "run_id": panic_run, "location": location}),
        ) {
            let _ = fs::write(panic_directory.join("last-panic.json"), bytes);
        }
        previous(info);
    }));
    tracing::info!(%run_id, application, version = env!("CARGO_PKG_VERSION"), event = "startup");
    Ok(Logging {
        run_id,
        directory,
        failures,
        dropped,
        _guard: guard,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotation_bounds_total_files_and_preserves_recent_events() {
        let temp = tempfile::tempdir().unwrap();
        let mut writer = RollingWriter {
            directory: temp.path().to_path_buf(),
            failures: Arc::new(AtomicU64::new(0)),
        };
        let record = vec![b'x'; 6 * 1024 * 1024];
        for _ in 0..8 {
            writer.write_all(&record).unwrap();
        }
        let files = fs::read_dir(temp.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|f| f.path().extension().is_some_and(|e| e == "jsonl"))
            .count();
        assert_eq!(files, FILES);
        assert_eq!(writer.failures.load(Ordering::Relaxed), 0);
    }
    #[test]
    fn io_failure_is_counted_without_recursion() {
        let mut writer = RollingWriter {
            directory: PathBuf::from("/nonexistent/coflow/log/path"),
            failures: Arc::new(AtomicU64::new(0)),
        };
        assert!(writer.write_all(b"event").is_err());
        assert_eq!(writer.failures.load(Ordering::Relaxed), 1);
    }
}
