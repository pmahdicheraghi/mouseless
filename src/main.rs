//! mouseless for Windows: control the mouse with the keyboard.
//!
//! A port of mouseless (https://github.com/jbensmann/mouseless) to Windows, using the same config format.

mod config;
mod engine;
mod keys;
mod logger;
#[cfg(windows)]
mod platform;

use std::path::PathBuf;
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
      --hidden            Hide the console window and keep running in the background
      --log-file <FILE>   Write the log to a file instead of the console
      --process-injected  Also handle keys sent by other programs
                          (e.g. remote desktop tools or on-screen keyboards)
      --list-keys         List all key names that can be used in the config
      --check             Only check the config file for errors and exit
  -v, --version           Show the version
  -h, --help              Show this help
";

struct Options {
    config: Option<PathBuf>,
    debug: bool,
    hidden: bool,
    log_file: Option<PathBuf>,
    check: bool,
    process_injected: bool,
}

fn main() -> ExitCode {
    let options = match parse_args() {
        Ok(Some(o)) => o,
        Ok(None) => return ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}\n\n{HELP}");
            return ExitCode::FAILURE;
        }
    };

    logger::set_debug(options.debug);
    if let Some(path) = &options.log_file {
        match std::fs::OpenOptions::new().create(true).append(true).open(path) {
            Ok(f) => logger::set_log_file(f),
            Err(e) => {
                eprintln!("error: failed to open the log file {}: {e}", path.display());
                return ExitCode::FAILURE;
            }
        }
    }
    logger::start_writer_thread();

    let result = run(options);
    if let Err(e) = &result {
        error!("{e}");
        error!("Exiting");
    }
    logger::flush();
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => {
            wait_for_enter_if_own_console();
            ExitCode::FAILURE
        }
    }
}

fn run(options: Options) -> Result<(), String> {
    let config_path = match options.config {
        Some(p) => p,
        None => default_config_path()?,
    };
    info!("Using config file: {}", config_path.display());
    let config = config::read_config(&config_path).map_err(|e| format!("Failed to read the config file: {e}"))?;

    if options.check {
        info!("The config file is valid ({} layers)", config.layers.len());
        return Ok(());
    }

    #[cfg(windows)]
    {
        platform::set_process_injected(options.process_injected);
        platform::run(config_path, config, options.hidden)
    }
    #[cfg(not(windows))]
    {
        let _ = (config, options.hidden, options.process_injected);
        Err("this version of mouseless only runs on Windows".into())
    }
}

fn parse_args() -> Result<Option<Options>, String> {
    let mut options = Options { config: None, debug: false, hidden: false, log_file: None, check: false, process_injected: false };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--config" => options.config = Some(args.next().ok_or("--config requires a file")?.into()),
            "-d" | "--debug" => options.debug = true,
            "--hidden" => options.hidden = true,
            "--log-file" => options.log_file = Some(args.next().ok_or("--log-file requires a file")?.into()),
            "--check" => options.check = true,
            "--process-injected" => options.process_injected = true,
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

/// When started by a double click, the console window closes immediately on exit, so wait for the user
/// to be able to read the error.
fn wait_for_enter_if_own_console() {
    #[cfg(windows)]
    if platform::has_own_console() {
        println!("Press Enter to exit...");
        let _ = std::io::stdin().read_line(&mut String::new());
    }
}
