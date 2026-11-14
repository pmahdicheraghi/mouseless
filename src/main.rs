//! mouseless for Windows: control the mouse with the keyboard.
//!
//! A port of mouseless (https://github.com/jbensmann/mouseless) to Windows, using the same config format.

// a GUI application without a console window; when started from a terminal, it attaches to its console
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

#[cfg(windows)]
mod autostart;
mod config;
#[cfg(windows)]
mod console;
mod engine;
mod keys;
mod logger;
#[cfg(windows)]
mod platform;
#[cfg(windows)]
mod tray;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_CONFIG: &str = include_str!("../examples/config_minimal.yaml");

const HELP: &str = "\
mouseless - control the mouse with the keyboard (Windows port)

Usage: mouseless [OPTIONS]

Options:
  -c, --config <FILE>     Use this config file
                          (default: %APPDATA%\\mouseless\\config.yaml, created if missing)
  -d, --debug             Show verbose debug information, e.g. the names of pressed keys
      --log-file <FILE>   Write the log to this file (default: the console when started from a
                          terminal, otherwise %LOCALAPPDATA%\\mouseless\\mouseless.log)
      --no-tray           Don't show the tray icon
      --process-injected  Also handle keys sent by other programs
                          (e.g. remote desktop tools or on-screen keyboards)
      --list-keys         List all key names that can be used in the config
      --check             Only check the config file for errors and exit
      --quit              Stop the running mouseless
      --enable-autostart  Start mouseless at login with administrator rights (asks for them)
      --disable-autostart Don't start mouseless at login anymore
  -v, --version           Show the version
  -h, --help              Show this help
";

#[derive(PartialEq)]
enum Mode {
    Run,
    Check,
    Quit,
    EnableAutostart,
    DisableAutostart,
}

struct Options {
    mode: Mode,
    config: Option<PathBuf>,
    debug: bool,
    log_file: Option<PathBuf>,
    tray: bool,
    process_injected: bool,
}

fn main() -> ExitCode {
    #[cfg(windows)]
    let has_console = console::attach();
    #[cfg(not(windows))]
    let has_console = true;

    let options = match parse_args() {
        Ok(Some(o)) => o,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            show_error(&format!("{e}\n\nRun 'mouseless --help' to see all options."), has_console);
            return ExitCode::FAILURE;
        }
    };

    logger::set_debug(options.debug);
    let log_file = options.log_file.clone().or_else(|| {
        // without a console, log to a file when running
        (options.mode == Mode::Run && !has_console).then(default_log_file).flatten()
    });
    if let Some(path) = &log_file {
        let append = options.log_file.is_some();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(path);
        match file {
            Ok(f) => logger::set_log_file(f, path.to_path_buf()),
            Err(e) => {
                show_error(&format!("Failed to open the log file {}: {e}", path.display()), has_console);
                return ExitCode::FAILURE;
            }
        }
    }
    logger::start_writer_thread();

    let result = run(options);
    if let Err(e) = &result {
        error!("{e}");
    }
    logger::flush();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            if !has_console {
                show_error(&e, false);
            }
            ExitCode::FAILURE
        }
    }
}

fn run(options: Options) -> Result<(), String> {
    #[cfg(windows)]
    match options.mode {
        Mode::Quit => {
            if tray::quit_running_instance() {
                info!("mouseless has been stopped");
            } else {
                info!("mouseless is not running");
            }
            return Ok(());
        }
        Mode::EnableAutostart | Mode::DisableAutostart => {
            // the task starts mouseless with the same config file, if one was given
            if let Some(c) = &options.config {
                let abs = std::path::absolute(c).unwrap_or_else(|_| c.clone());
                autostart::set_task_args(vec!["--config".into(), abs.to_string_lossy().into_owned()]);
            }
            return if options.mode == Mode::EnableAutostart {
                autostart::enable().map(|()| info!("Autostart enabled"))
            } else {
                autostart::disable().map(|()| info!("Autostart disabled"))
            };
        }
        _ => {}
    }

    let config_path = match &options.config {
        Some(p) => std::path::absolute(p).unwrap_or_else(|_| p.clone()),
        None => default_config_path()?,
    };
    info!("Using config file: {}", config_path.display());
    let config = config::read_config(&config_path).map_err(|e| format!("Failed to read the config file: {e}"))?;

    if options.mode == Mode::Check {
        info!("The config file is valid ({} layers)", config.layers.len());
        return Ok(());
    }

    #[cfg(windows)]
    {
        if options.config.is_some() {
            // enabling the autostart from the tray menu keeps the config file
            autostart::set_task_args(vec!["--config".into(), config_path.to_string_lossy().into_owned()]);
        }
        platform::set_process_injected(options.process_injected);
        platform::run(config_path, config, options.tray)
    }
    #[cfg(not(windows))]
    {
        let _ = (config, options.tray, options.process_injected);
        Err("this version of mouseless only runs on Windows".into())
    }
}

fn parse_args() -> Result<Option<Options>, String> {
    let mut options = Options {
        mode: Mode::Run,
        config: None,
        debug: false,
        log_file: None,
        tray: true,
        process_injected: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--config" => options.config = Some(args.next().ok_or("--config requires a file")?.into()),
            "-d" | "--debug" => options.debug = true,
            "--log-file" => options.log_file = Some(args.next().ok_or("--log-file requires a file")?.into()),
            "--no-tray" => options.tray = false,
            "--process-injected" => options.process_injected = true,
            // the console window of older versions, not needed anymore
            "--hidden" => {}
            "--check" => options.mode = Mode::Check,
            "--quit" => options.mode = Mode::Quit,
            "--enable-autostart" => options.mode = Mode::EnableAutostart,
            "--disable-autostart" => options.mode = Mode::DisableAutostart,
            "--list-keys" => {
                keys::print_key_names();
                return Ok(None);
            }
            "-v" | "--version" => {
                println!("{VERSION}");
                return Ok(None);
            }
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(None);
            }
            _ => {
                if let Some(path) = arg.strip_prefix("--config=") {
                    options.config = Some(path.into());
                } else {
                    return Err(format!("unknown argument '{arg}'"));
                }
            }
        }
    }
    Ok(Some(options))
}

/// Returns the default config file, `%APPDATA%\mouseless\config.yaml`. If neither it nor the config file
/// of the Linux version (`~/.config/mouseless/config.yaml`) exists, a default config is created.
fn default_config_path() -> Result<PathBuf, String> {
    let home_config = std::env::var_os("USERPROFILE")
        .map(|h| PathBuf::from(h).join(".config").join("mouseless").join("config.yaml"));
    let app_data = std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("mouseless").join("config.yaml"));

    if let Some(p) = &app_data {
        if p.exists() {
            return Ok(p.clone());
        }
    }
    if let Some(p) = &home_config {
        if p.exists() {
            return Ok(p.clone());
        }
    }
    let path = app_data.or(home_config).ok_or("cannot determine the config directory, use --config")?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("failed to create {}: {e}", dir.display()))?;
    }
    std::fs::write(&path, DEFAULT_CONFIG).map_err(|e| format!("failed to create {}: {e}", path.display()))?;
    info!("Created a default config file: {}", path.display());
    Ok(path)
}

fn default_log_file() -> Option<PathBuf> {
    let dir = std::env::var_os("LOCALAPPDATA")?;
    Some(Path::new(&dir).join("mouseless").join("mouseless.log"))
}

/// Shows an error on the console, or in a message box if there is none.
fn show_error(msg: &str, has_console: bool) {
    if has_console {
        eprintln!("error: {msg}");
    } else {
        #[cfg(windows)]
        tray::message_box(msg, true);
    }
}
