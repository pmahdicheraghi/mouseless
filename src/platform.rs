//! The Windows specific part: a low-level keyboard hook reads (and suppresses) the key events, and
//! SendInput is used to emit keys, mouse movements, clicks and scrolling.
//!
//! Threads:
//! - the main thread installs the keyboard hook and runs the message loop; the hook callback feeds
//!   the key events synchronously into the engine and decides whether the original event is
//!   suppressed or passed through
//! - the timer thread drives the time based parts of the engine (tap-hold and combo timeouts,
//!   mouse movement, repeated clicks)
//!
//! The main thread also owns the hidden window with the tray icon (see `tray.rs`).
//!
//! The engine is protected by a mutex that is only held for short amounts of time, since Windows
//! removes keyboard hooks that take too long.

use crate::config::{self, Config, MouseButton};
use crate::engine::{Engine, Event, Output};
use crate::keys;
use crate::{debug, error, info, warn};
use std::collections::HashSet;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicPtr, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Media::{timeBeginPeriod, timeEndPeriod};
use windows_sys::Win32::System::Console::{
    SetConsoleCtrlHandler, CTRL_CLOSE_EVENT, CTRL_LOGOFF_EVENT, CTRL_SHUTDOWN_EVENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{CreateMutexW, GetCurrentThreadId, CREATE_NO_WINDOW};
use windows_sys::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, PostMessageW, PostThreadMessageW,
    SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, LLKHF_EXTENDED,
    LLKHF_INJECTED, MSG, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
    WH_KEYBOARD_LL, WM_CLOSE, WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN, XBUTTON1, XBUTTON2,
};

/// Marks the input events created by mouseless.
const INJECTED_TAG: usize = 0x6D6C_7373; // "mlss"

struct Shared {
    state: Mutex<State>,
    /// wakes up the timer thread when the next deadline of the engine might have changed
    wakeup: Condvar,
    main_thread_id: u32,
    config_path: PathBuf,
}

struct State {
    engine: Engine,
    /// the physical keys that are currently held down, used to detect auto-repeat
    physically_pressed: HashSet<u16>,
    /// false while mouseless is paused (all keys are passed through)
    enabled: bool,
    /// the layer that was active after the last event, to detect layer changes
    last_layer: usize,
    quitting: bool,
}

static SHARED: OnceLock<Shared> = OnceLock::new();

/// The hidden window, which is informed about state changes (to update the tray icon).
static WINDOW: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

/// Whether key events injected by other programs are processed as well (e.g. from remote desktop
/// tools or on-screen keyboards). The events created by mouseless itself are always ignored.
static PROCESS_INJECTED: AtomicBool = AtomicBool::new(false);

pub fn set_process_injected(enabled: bool) {
    PROCESS_INJECTED.store(enabled, Ordering::Relaxed);
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Runs mouseless until it is stopped (Ctrl+C, the quit action, the tray menu, or `mouseless --quit`).
pub fn run(config_path: PathBuf, config: Config, show_tray: bool) -> Result<(), String> {
    let instance_name = if config.instance_name.is_empty() { "mouseless" } else { &config.instance_name };
    ensure_single_instance(instance_name)?;

    unsafe {
        // get the real pixel coordinates on displays with scaling
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        // precise timers for a smooth mouse movement
        timeBeginPeriod(1);
    }

    if !config.devices.is_empty() {
        warn!("The options 'devices' and 'devicesExclude' are not supported on Windows, all keyboards are used");
    }
    if !config.start_command.is_empty() {
        debug!("Executing start command: {}", config.start_command);
        match shell_command(&config.start_command).status() {
            Ok(s) if s.success() => {}
            Ok(s) => warn!("Execution of start command failed: {s}"),
            Err(e) => warn!("Execution of start command failed: {e}"),
        }
    }

    let first_layer = config.layers[0].name.clone();
    let shared = Shared {
        state: Mutex::new(State {
            engine: Engine::new(config),
            physically_pressed: HashSet::new(),
            enabled: true,
            last_layer: 0,
            quitting: false,
        }),
        wakeup: Condvar::new(),
        main_thread_id: unsafe { GetCurrentThreadId() },
        config_path,
    };
    if SHARED.set(shared).is_err() {
        return Err("already running".into());
    }
    let shared = SHARED.get().expect("just set");

    // Ctrl+C when started from a terminal
    unsafe {
        SetConsoleCtrlHandler(Some(console_ctrl_handler), 1);
    }

    let timer_thread = std::thread::Builder::new()
        .name("timer".into())
        .spawn(timer_loop)
        .map_err(|e| format!("failed to start the timer thread: {e}"))?;

    let hwnd = crate::tray::create(show_tray)?;
    WINDOW.store(hwnd, Ordering::Relaxed);

    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), GetModuleHandleW(std::ptr::null()), 0) };
    if hook.is_null() {
        return Err(format!("failed to install the keyboard hook (error {})", unsafe { GetLastError() }));
    }

    let admin = if crate::autostart::is_elevated() { " as administrator" } else { "" };
    info!("mouseless is running{admin} (layer '{first_layer}')");

    // the message loop, which is required for the keyboard hook
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        UnhookWindowsHookEx(hook);
    }
    WINDOW.store(std::ptr::null_mut(), Ordering::Relaxed);
    crate::tray::destroy();

    // release everything that is still pressed, so that no key gets stuck
    {
        let mut state = shared.lock();
        state.quitting = true;
        let outputs = state.engine.release_all();
        send_outputs(&outputs);
    }
    shared.wakeup.notify_all();
    let _ = timer_thread.join();
    unsafe { timeEndPeriod(1) };
    info!("Exiting");
    Ok(())
}

fn ensure_single_instance(instance_name: &str) -> Result<(), String> {
    let name = wide(&format!("Local\\mouseless-{instance_name}"));
    let handle: HANDLE = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
    if handle.is_null() {
        return Err(format!("failed to create the instance mutex (error {})", unsafe { GetLastError() }));
    }
    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        return Err("mouseless is already running (see the tray icon). If you want to run multiple instances, \
                    give them different instanceNames in the config."
            .into());
    }
    // the handle is intentionally leaked, the mutex lives as long as the process
    Ok(())
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
        let is_down = matches!(wparam as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        // never let a panic escape into Windows, and pass the key through in that case
        let suppress = std::panic::catch_unwind(|| handle_key(info, is_down)).unwrap_or(false);
        if suppress {
            return 1;
        }
    }
    unsafe { CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam) }
}

/// Handles a key event from the keyboard hook, returns whether the original event shall be suppressed.
fn handle_key(info: &KBDLLHOOKSTRUCT, is_down: bool) -> bool {
    // ignore our own events, and by default also those injected by other programs
    let injected = info.flags & LLKHF_INJECTED != 0;
    if info.dwExtraInfo == INJECTED_TAG || (injected && !PROCESS_INJECTED.load(Ordering::Relaxed)) {
        return false;
    }
    // fake events generated by Windows, e.g. the left ctrl that is sent along with AltGr (scan code 0x21D)
    if info.scanCode > 0xFF {
        return false;
    }
    let extended = info.flags & LLKHF_EXTENDED != 0;
    let Some(code) = keys::from_windows(info.vkCode, info.scanCode, extended) else {
        debug!(
            "Ignoring unknown key: vk={:#04x} scan={:#04x} extended={}",
            info.vkCode, info.scanCode, extended
        );
        return false;
    };
    let Some(shared) = SHARED.get() else { return false };

    let mut state = shared.lock();
    if state.quitting || !state.enabled {
        return false;
    }
    let outputs = if is_down {
        if state.physically_pressed.insert(code) {
            debug!("Pressed:  {} ({})", keys::key_name_or_unknown(code), code);
            state.engine.handle_event(Event { code, is_press: true, time: Instant::now() })
        } else {
            state.engine.handle_repeat(code)
        }
    } else {
        if !state.physically_pressed.remove(&code) {
            // the key has been pressed before mouseless started
            return false;
        }
        debug!("Released: {} ({})", keys::key_name_or_unknown(code), code);
        state.engine.handle_event(Event { code, is_press: false, time: Instant::now() })
    };

    // if the result is exactly the original event, it is simply passed through
    let pass_through = matches!(outputs.as_slice(), [Output::Key { code: c, down }] if *c == code && *down == is_down);
    if !pass_through {
        send_outputs(&outputs);
    }
    after_outputs(shared, &mut state);
    drop(state);
    shared.wakeup.notify_all();
    !pass_through
}

/// Handles requests of the engine that have to be executed outside of it.
fn after_outputs(shared: &Shared, state: &mut State) {
    if state.engine.reload_requested {
        state.engine.reload_requested = false;
        info!("Reloading the config file: {}", shared.config_path.display());
        match config::read_config(&shared.config_path) {
            Ok(c) => state.engine.set_config(c),
            Err(e) => warn!("Failed to read the config file: {e}"),
        }
    }
    if state.engine.quit_requested {
        state.engine.quit_requested = false;
        info!("Quit requested");
        request_quit(shared);
    }
    let (layer, name) = state.engine.current_layer_info();
    if layer != state.last_layer {
        debug!("Layer changed to '{name}'");
        state.last_layer = layer;
        notify_window();
    }
}

/// Stops mouseless: via the window if it exists (which also closes an open tray menu), otherwise by
/// ending the message loop directly.
fn request_quit(shared: &Shared) {
    let hwnd: HWND = WINDOW.load(Ordering::Relaxed);
    unsafe {
        if hwnd.is_null() || PostMessageW(hwnd, WM_CLOSE, 0, 0) == 0 {
            PostThreadMessageW(shared.main_thread_id, WM_QUIT, 0, 0);
        }
    }
}

/// Informs the window (tray icon) that the state has changed.
fn notify_window() {
    let hwnd: HWND = WINDOW.load(Ordering::Relaxed);
    if !hwnd.is_null() {
        unsafe { PostMessageW(hwnd, crate::tray::WM_STATE_CHANGED, 0, 0) };
    }
}

fn shared() -> &'static Shared {
    SHARED.get().expect("mouseless is running")
}

/// Returns the index and name of the current layer.
pub fn current_layer() -> (usize, String) {
    let state = shared().lock();
    let (index, name) = state.engine.current_layer_info();
    (index, name.to_string())
}

pub fn is_enabled() -> bool {
    shared().lock().enabled
}

/// Pauses or resumes mouseless. While paused, all keys are passed through unchanged.
pub fn set_enabled(enabled: bool) {
    let shared = shared();
    let mut state = shared.lock();
    if state.enabled == enabled {
        return;
    }
    info!("{}", if enabled { "Enabled" } else { "Paused" });
    state.enabled = enabled;
    if !enabled {
        let outputs = state.engine.reset();
        send_outputs(&outputs);
        // releases of keys that are held now are passed through
        state.physically_pressed.clear();
    }
    state.last_layer = state.engine.current_layer_info().0;
    drop(state);
    shared.wakeup.notify_all();
    notify_window();
}

/// Reloads the config file.
pub fn request_reload() {
    let shared = shared();
    let mut state = shared.lock();
    state.engine.reload_requested = true;
    after_outputs(shared, &mut state);
    drop(state);
    shared.wakeup.notify_all();
}

pub fn config_path() -> PathBuf {
    shared().config_path.clone()
}

/// Executes the timeouts and mouse movements of the engine.
fn timer_loop() {
    let shared = SHARED.get().expect("initialized before the thread is started");
    let mut state = shared.lock();
    loop {
        if state.quitting {
            return;
        }
        let now = Instant::now();
        if state.engine.next_deadline().is_some_and(|d| d <= now) {
            let outputs = state.engine.on_timer(now);
            send_outputs(&outputs);
            after_outputs(shared, &mut state);
        }
        state = match state.engine.next_deadline() {
            None => shared.wakeup.wait(state).unwrap_or_else(|e| e.into_inner()),
            Some(deadline) => {
                let now = Instant::now();
                if deadline <= now {
                    continue;
                }
                shared.wakeup.wait_timeout(state, deadline - now).unwrap_or_else(|e| e.into_inner()).0
            }
        };
    }
}

unsafe extern "system" fn console_ctrl_handler(ctrl_type: u32) -> windows_sys::core::BOOL {
    if let Some(shared) = SHARED.get() {
        request_quit(shared);
        if matches!(ctrl_type, CTRL_CLOSE_EVENT | CTRL_LOGOFF_EVENT | CTRL_SHUTDOWN_EVENT) {
            // the process is terminated when this function returns, so give the main thread some
            // time to release all keys
            std::thread::sleep(Duration::from_millis(500));
        }
    }
    1
}

// -------------------------------------------------------------------------------------------------
// output
// -------------------------------------------------------------------------------------------------

/// Executes the given outputs of the engine.
fn send_outputs(outputs: &[Output]) {
    let mut inputs: Vec<INPUT> = Vec::new();
    let mut cursor: Option<POINT> = None;
    for output in outputs {
        match output {
            Output::Key { code, down } => match keys::windows_key(*code) {
                Some(key) => inputs.push(key_input(key, *down)),
                None => warn!("The key {} ({}) cannot be sent on Windows", keys::key_name_or_unknown(*code), code),
            },
            Output::MouseMove { dx, dy } => {
                let pos = cursor.get_or_insert_with(cursor_position);
                pos.x += dx;
                pos.y += dy;
                inputs.push(mouse_move_input(pos));
            }
            Output::Wheel { horizontal, delta } => {
                let flags = if *horizontal { MOUSEEVENTF_HWHEEL } else { MOUSEEVENTF_WHEEL };
                inputs.push(mouse_input(0, 0, *delta as u32, flags));
            }
            Output::Button { button, down } => {
                let (flags, data) = match (button, down) {
                    (MouseButton::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
                    (MouseButton::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
                    (MouseButton::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
                    (MouseButton::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
                    (MouseButton::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
                    (MouseButton::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
                    (MouseButton::X1, true) => (MOUSEEVENTF_XDOWN, XBUTTON1),
                    (MouseButton::X1, false) => (MOUSEEVENTF_XUP, XBUTTON1),
                    (MouseButton::X2, true) => (MOUSEEVENTF_XDOWN, XBUTTON2),
                    (MouseButton::X2, false) => (MOUSEEVENTF_XUP, XBUTTON2),
                };
                inputs.push(mouse_input(0, 0, data as u32, flags));
            }
            Output::Exec { command, env } => {
                // keep the order of inputs and commands
                send_inputs(&mut inputs);
                execute_command(command.clone(), env.clone());
            }
        }
    }
    send_inputs(&mut inputs);
}

fn send_inputs(inputs: &mut Vec<INPUT>) {
    if inputs.is_empty() {
        return;
    }
    let sent = unsafe { SendInput(inputs.len() as u32, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != inputs.len() {
        // this happens e.g. if the foreground window belongs to a process with higher privileges
        debug!("SendInput: only {} of {} events were sent (error {})", sent, inputs.len(), unsafe { GetLastError() });
    }
    inputs.clear();
}

fn key_input(key: keys::WinKey, down: bool) -> INPUT {
    let mut flags = if down { 0 } else { KEYEVENTF_KEYUP };
    if key.extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    let (vk, scan) = if key.vk != 0 {
        (key.vk, key.scan)
    } else {
        // send the scan code, so that the key is independent of the keyboard layout
        flags |= KEYEVENTF_SCANCODE;
        (0, key.scan)
    };
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: INJECTED_TAG },
        },
    }
}

fn mouse_input(dx: i32, dy: i32, data: u32, flags: u32) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx, dy, mouseData: data, dwFlags: flags, time: 0, dwExtraInfo: INJECTED_TAG },
        },
    }
}

fn cursor_position() -> POINT {
    let mut p = POINT { x: 0, y: 0 };
    unsafe { GetCursorPos(&mut p) };
    p
}

/// Creates an absolute mouse movement to the given position.
///
/// Absolute movements are used since relative ones are affected by the mouse speed and the
/// "enhance pointer precision" setting of Windows.
fn mouse_move_input(pos: &mut POINT) -> INPUT {
    let (left, top, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1),
            GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1),
        )
    };
    pos.x = pos.x.clamp(left, left + width - 1);
    pos.y = pos.y.clamp(top, top + height - 1);
    // the coordinates are normalized to 0..65535 over the whole virtual desktop
    let normalize = |v: i32, size: i32| ((v as i64 * 65536 + 65536 / 2) / size as i64) as i32;
    mouse_input(
        normalize(pos.x - left, width),
        normalize(pos.y - top, height),
        0,
        MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
    )
}

fn shell_command(command: &str) -> Command {
    let mut cmd = Command::new("cmd");
    // with /S, cmd removes the outer quotes and executes the rest as it is
    cmd.arg("/S").arg("/C").raw_arg(format!("\"{command}\"")).creation_flags(CREATE_NO_WINDOW);
    cmd
}

/// Executes the given command in the background.
fn execute_command(command: String, env: Vec<(String, String)>) {
    debug!("Executing command: {}", command);
    std::thread::spawn(move || {
        let result = shell_command(&command)
            .envs(env)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output();
        match result {
            Ok(out) if !out.status.success() => warn!(
                "Execution of command '{}' failed: {}, stderr: {}",
                command,
                out.status,
                String::from_utf8_lossy(&out.stderr).trim()
            ),
            Err(e) => error!("Execution of command '{}' failed: {}", command, e),
            _ => {}
        }
    });
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
