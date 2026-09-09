//! 极简文件日志：不引入新依赖，只为支持与排障提供时间线。
//!
//! 日志只记录阶段、错误类别与耗时，不记录请求体、密钥、账号或路径内容。

use std::{
    fs::{create_dir_all, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::Mutex,
};

const MAX_BYTES: u64 = 4 * 1024 * 1024;

struct FileLogger {
    file: Mutex<std::fs::File>,
}

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let Ok(mut file) = self.file.lock() else {
            return;
        };
        let _ = writeln!(
            file,
            "{} {:<5} {} {}",
            timestamp(),
            record.level(),
            record.target(),
            record.args()
        );
    }

    fn flush(&self) {
        if let Ok(mut file) = self.file.lock() {
            let _ = file.flush();
        }
    }
}

fn timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}", now.as_secs(), now.subsec_millis())
}

fn log_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join("Library")
                .join("Logs")
                .join("野菜API")
                .join("yeschoy.log"),
        )
    }
    #[cfg(target_os = "windows")]
    {
        let local = std::env::var_os("LOCALAPPDATA")?;
        Some(
            PathBuf::from(local)
                .join("野菜API")
                .join("logs")
                .join("yeschoy.log"),
        )
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let home = std::env::var_os("HOME")?;
        Some(
            PathBuf::from(home)
                .join(".local")
                .join("state")
                .join("野菜API")
                .join("yeschoy.log"),
        )
    }
}

/// 安装日志器；失败时静默跳过（日志缺失不应影响启动）。
pub(crate) fn init() {
    let Some(path) = log_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        if create_dir_all(parent).is_err() {
            return;
        }
    }
    // 单文件上限：超过后从空文件重新开始，避免无限增长。
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > MAX_BYTES) {
        let _ = std::fs::remove_file(&path);
    }
    let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) else {
        return;
    };
    static LOGGER: std::sync::OnceLock<FileLogger> = std::sync::OnceLock::new();
    let logger = LOGGER.get_or_init(|| FileLogger {
        file: Mutex::new(file),
    });
    if log::set_logger(logger).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
        log::info!(
            "yeschoy_desktop stage=start version={} log={}",
            env!("CARGO_PKG_VERSION"),
            path.display()
        );
    }
}
