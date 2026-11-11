//! A minimal logger that writes timestamped lines to stdout or a log file.
//!
//! The lines are written by a background thread, so that logging never blocks the keyboard hook
//! (e.g. writing to a console blocks while text in the console window is selected).

use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, sync_channel, Sender, SyncSender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

static DEBUG: AtomicBool = AtomicBool::new(false);
static LOG_FILE: Mutex<Option<File>> = Mutex::new(None);
static WRITER: OnceLock<Mutex<Sender<Message>>> = OnceLock::new();

enum Message {
    Line(String),
    Flush(SyncSender<()>),
}

/// Starts the background thread that writes the log lines.
pub fn start_writer_thread() {
    let (tx, rx) = channel::<Message>();
    if WRITER.set(Mutex::new(tx)).is_err() {
        return;
    }
    std::thread::spawn(move || {
        for msg in rx {
            match msg {
                Message::Line(line) => write_line(&line),
                Message::Flush(done) => {
                    let _ = done.send(());
                }
            }
        }
    });
}

/// Waits (for a short time) until all log lines have been written.
pub fn flush() {
    if let Some(w) = WRITER.get() {
        let (tx, rx) = sync_channel(1);
        let sent = w.lock().map(|s| s.send(Message::Flush(tx)).is_ok()).unwrap_or(false);
        if sent {
            let _ = rx.recv_timeout(Duration::from_secs(1));
        }
    }
}

pub fn set_debug(enabled: bool) {
    DEBUG.store(enabled, Ordering::Relaxed);
}

pub fn debug_enabled() -> bool {
    DEBUG.load(Ordering::Relaxed)
}

pub fn set_log_file(file: File) {
    *LOG_FILE.lock().unwrap_or_else(|e| e.into_inner()) = Some(file);
}

pub fn log(level: &str, msg: std::fmt::Arguments) {
    let line = format!("{} {:<5} {}\n", timestamp(), level, msg);
    if let Some(w) = WRITER.get() {
        if let Ok(sender) = w.lock() {
            if sender.send(Message::Line(line.clone())).is_ok() {
                return;
            }
        }
    }
    write_line(&line);
}

fn write_line(line: &str) {
    let mut file = LOG_FILE.lock().unwrap_or_else(|e| e.into_inner());
    match file.as_mut() {
        Some(f) => {
            let _ = f.write_all(line.as_bytes());
        }
        None => {
            let _ = std::io::stdout().lock().write_all(line.as_bytes());
        }
    }
}

#[cfg(windows)]
fn timestamp() -> String {
    let mut t = unsafe { std::mem::zeroed::<windows_sys::Win32::Foundation::SYSTEMTIME>() };
    unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t) };
    format!("{:02}:{:02}:{:02}.{:03}", t.wHour, t.wMinute, t.wSecond, t.wMilliseconds)
}

#[cfg(not(windows))]
fn timestamp() -> String {
    let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("{}.{:03}", d.as_secs(), d.subsec_millis())
}

#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => {
        if $crate::logger::debug_enabled() {
            $crate::logger::log("DEBUG", format_args!($($arg)*))
        }
    };
}

#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => { $crate::logger::log("INFO", format_args!($($arg)*)) };
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => { $crate::logger::log("WARN", format_args!($($arg)*)) };
}

#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => { $crate::logger::log("ERROR", format_args!($($arg)*)) };
}
