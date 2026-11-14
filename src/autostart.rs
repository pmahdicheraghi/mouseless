//! Starting mouseless automatically at login, with administrator rights.
//!
//! This is done with a scheduled task (like `Register-ScheduledTask` in PowerShell), since programs in
//! the Startup folder cannot be started elevated. Creating such a task requires administrator rights,
//! so if mouseless is not elevated, it starts itself elevated (UAC prompt) to do it.

use crate::debug;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_CANCELLED, HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows_sys::Win32::System::Threading::{
    GetCurrentProcess, GetExitCodeProcess, OpenProcessToken, WaitForSingleObject, CREATE_NO_WINDOW,
};
use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOASYNC, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

pub const TASK_NAME: &str = "mouseless";

/// Additional arguments for mouseless when started by the task (e.g. `--config <file>`).
static TASK_ARGS: OnceLock<Vec<String>> = OnceLock::new();

pub fn set_task_args(args: Vec<String>) {
    let _ = TASK_ARGS.set(args);
}

fn task_args() -> &'static [String] {
    TASK_ARGS.get().map(|a| a.as_slice()).unwrap_or(&[])
}

/// Returns whether this process runs with administrator rights.
pub fn is_elevated() -> bool {
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut len = 0;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            &mut elevation as *mut _ as *mut _,
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        );
        CloseHandle(token);
        ok != 0 && elevation.TokenIsElevated != 0
    }
}

/// Returns whether the autostart task exists.
pub fn is_enabled() -> bool {
    Command::new("schtasks")
        .args(["/Query", "/TN", TASK_NAME])
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Creates (or replaces) the autostart task for the current user, which starts this executable.
pub fn enable() -> Result<(), String> {
    if !is_elevated() {
        let mut args = vec!["--enable-autostart".to_string()];
        args.extend(task_args().iter().cloned());
        return run_self_elevated(&args);
    }
    let exe = std::env::current_exe().map_err(|e| format!("cannot determine the executable: {e}"))?;
    let xml = task_xml(&exe, task_args());
    let path = std::env::temp_dir().join(format!("mouseless-task-{}.xml", std::process::id()));
    // schtasks expects the XML as UTF-16
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(xml.encode_utf16().flat_map(|c| c.to_le_bytes()));
    std::fs::write(&path, bytes).map_err(|e| format!("failed to write {}: {e}", path.display()))?;
    let result = schtasks(&["/Create", "/TN", TASK_NAME, "/XML", &path.to_string_lossy(), "/F"]);
    let _ = std::fs::remove_file(&path);
    result
}

/// Deletes the autostart task.
pub fn disable() -> Result<(), String> {
    if !is_elevated() {
        return run_self_elevated(&["--disable-autostart".to_string()]);
    }
    if !is_enabled() {
        return Ok(());
    }
    schtasks(&["/Delete", "/TN", TASK_NAME, "/F"])
}

fn schtasks(args: &[&str]) -> Result<(), String> {
    debug!("Running schtasks {:?}", args);
    let out = Command::new("schtasks")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("failed to run schtasks: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        let msg = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(format!("schtasks failed: {msg}"))
    }
}

/// Starts this executable with administrator rights (shows the UAC prompt) and waits for it to finish.
fn run_self_elevated(args: &[String]) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot determine the executable: {e}"))?;
    let params = args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
    let verb = wide("runas");
    let file = wide(&exe.to_string_lossy());
    let params = wide(&params);
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params.as_ptr();
        info.nShow = SW_HIDE;
        if ShellExecuteExW(&mut info) == 0 {
            let err = GetLastError();
            return Err(if err == ERROR_CANCELLED {
                "administrator rights are required for this".to_string()
            } else {
                format!("failed to start mouseless as administrator (error {err})")
            });
        }
        if info.hProcess.is_null() {
            return Ok(());
        }
        let waited = WaitForSingleObject(info.hProcess, 60_000);
        let mut code = 1;
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        if waited != WAIT_OBJECT_0 {
            return Err("timed out".into());
        }
        if code != 0 {
            return Err(format!("the elevated mouseless failed with exit code {code}"));
        }
    }
    Ok(())
}

fn task_xml(exe: &Path, args: &[String]) -> String {
    let user = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").unwrap_or_default()
    );
    let user = xml_escape(&user);
    let command = xml_escape(&exe.to_string_lossy());
    let arguments = xml_escape(&args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" "));
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Starts mouseless at login with administrator rights.</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <IdleSettings>
      <StopOnIdleEnd>false</StopOnIdleEnd>
      <RestartOnIdle>false</RestartOnIdle>
    </IdleSettings>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>"{command}"</Command>
      <Arguments>{arguments}</Arguments>
    </Exec>
  </Actions>
</Task>
"#
    )
}

fn quote(arg: &str) -> String {
    if arg.is_empty() || arg.contains([' ', '\t', '"']) {
        format!("\"{}\"", arg.replace('"', "\\\""))
    } else {
        arg.to_string()
    }
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
