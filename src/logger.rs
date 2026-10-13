//! 日志出口: 从终端启动 (含 `-s`) 走 stdout; 双击启动没有控制台, 写 data 目录下的 clowder.log.
//! GUI 子系统程序的 stdout 句柄是从父终端继承的, 不需要 AttachConsole, 重定向和管道也都正常。

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

const MAX_LOG_BYTES: u64 = 1024 * 1024;

pub fn init(headless: bool) {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,clowder=debug"));
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false);

    if headless {
        install_panic_hook(None);
        builder.init();
        return;
    }

    match log_file() {
        Ok(file) => {
            install_panic_hook(Some(log_path()));
            builder.with_writer(file).init();
        }
        Err(e) => {
            // 日志文件都开不了的话, 至少别因为初始化日志而启动失败
            install_panic_hook(None);
            builder.init();
            eprintln!("cannot open log file: {e}");
        }
    }
}

pub fn log_path() -> PathBuf {
    clowder_core::data_dir()
        .map(|d| d.join("clowder.log"))
        .unwrap_or_else(|_| PathBuf::from("clowder.log"))
}

fn log_file() -> anyhow::Result<std::fs::File> {
    let path = log_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // 启动时清一次, 不让它无限涨
    if std::fs::metadata(&path).map(|m| m.len() > MAX_LOG_BYTES).unwrap_or(false) {
        std::fs::write(&path, b"")?;
    }
    Ok(OpenOptions::new().create(true).append(true).open(path)?)
}

/// 双击时进程没有控制台, panic 只会静默消失; 挂钩子把现场写进日志
fn install_panic_hook(log: Option<PathBuf>) {
    std::panic::set_hook(Box::new(move |info| {
        let msg = format!("panic: {info}\n{}{}\n", "", std::backtrace::Backtrace::capture());
        if let Some(path) = &log {
            if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
                let _ = f.write_all(msg.as_bytes());
                let _ = f.flush();
            }
        }
        eprint!("{msg}");
    }));
}

#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 启动就失败时 (配置读不了、端口被占) 双击只会"什么都没发生", 弹窗说明原因
#[cfg(windows)]
pub fn show_fatal(err: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR};
    let title = to_wide("Clowder 启动失败");
    let body = to_wide(err);
    unsafe {
        MessageBoxW(std::ptr::null_mut(), body.as_ptr(), title.as_ptr(), MB_ICONERROR);
    }
}

#[cfg(not(windows))]
pub fn show_fatal(err: &str) {
    eprintln!("fatal: {err}");
}
