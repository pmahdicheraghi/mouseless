//! The hidden window of mouseless and its tray icon.
//!
//! The window always exists, so that other processes (e.g. `mouseless --quit` or the installer) and
//! Windows itself can ask mouseless to exit gracefully. The tray icon is optional.

use crate::{autostart, debug, logger, platform, warn};
use std::cell::RefCell;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
use windows_sys::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
    NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

const WINDOW_CLASS: &str = "mouseless_window";
/// sent by the tray icon on mouse events
const WM_TRAY: u32 = WM_APP + 1;
/// sent by mouseless when the state (layer, enabled) has changed
pub const WM_STATE_CHANGED: u32 = WM_APP + 2;

const ICON_NORMAL: u16 = 1;
const ICON_ACTIVE: u16 = 2;
const ICON_PAUSED: u16 = 3;

const CMD_ENABLED: usize = 1;
const CMD_RELOAD: usize = 2;
const CMD_EDIT_CONFIG: usize = 3;
const CMD_OPEN_LOG: usize = 4;
const CMD_AUTOSTART: usize = 5;
const CMD_EXIT: usize = 6;

static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
/// cached state of the autostart task, since querying it takes a moment
static AUTOSTART_ENABLED: AtomicBool = AtomicBool::new(false);

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

struct Tray {
    hwnd: HWND,
    show_icon: bool,
    icon_added: bool,
    icons: [HICON; 3],
}

/// Creates the hidden window, and the tray icon if `show_icon` is set. Must be called on the thread
/// that runs the message loop.
pub fn create(show_icon: bool) -> Result<HWND, String> {
    unsafe {
        let hinstance = GetModuleHandleW(std::ptr::null());
        let class = wide(WINDOW_CLASS);
        let mut wc: WNDCLASSEXW = std::mem::zeroed();
        wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
        wc.lpfnWndProc = Some(wnd_proc);
        wc.hInstance = hinstance;
        wc.lpszClassName = class.as_ptr();
        wc.hIcon = LoadIconW(hinstance, ICON_NORMAL as usize as *const u16);
        if RegisterClassExW(&wc) == 0 {
            return Err("failed to register the window class".into());
        }
        let title = wide("mouseless");
        // a normal top-level window that is never shown, so that it receives WM_CLOSE from taskkill and
        // the session end messages
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            hinstance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            return Err("failed to create the window".into());
        }

        let taskbar_created = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
        TASKBAR_CREATED.store(taskbar_created, Ordering::Relaxed);
        // when running elevated, allow these messages from normal processes (explorer, mouseless --quit)
        for msg in [WM_CLOSE, WM_TRAY, taskbar_created] {
            ChangeWindowMessageFilterEx(hwnd, msg, MSGFLT_ALLOW, std::ptr::null_mut());
        }

        let size = GetSystemMetrics(SM_CXSMICON);
        let load = |id: u16| LoadImageW(hinstance, id as usize as *const u16, IMAGE_ICON, size, size, 0) as HICON;
        let tray = Tray {
            hwnd,
            show_icon,
            icon_added: false,
            icons: [load(ICON_NORMAL), load(ICON_ACTIVE), load(ICON_PAUSED)],
        };
        TRAY.with(|t| *t.borrow_mut() = Some(tray));
        if show_icon {
            update_icon();
            refresh_autostart_state();
        }
        Ok(hwnd)
    }
}

/// Removes the tray icon and the window.
pub fn destroy() {
    TRAY.with(|t| {
        if let Some(tray) = t.borrow_mut().take() {
            unsafe {
                if tray.icon_added {
                    let nid = notify_data(tray.hwnd);
                    Shell_NotifyIconW(NIM_DELETE, &nid);
                }
                DestroyWindow(tray.hwnd);
            }
        }
    });
}

/// Asks a running instance of mouseless to exit and waits until it has exited. Returns false if no
/// instance is running.
pub fn quit_running_instance() -> bool {
    unsafe {
        let class = wide(WINDOW_CLASS);
        let hwnd = FindWindowW(class.as_ptr(), std::ptr::null());
        if hwnd.is_null() {
            return false;
        }
        let mut pid = 0;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let process = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        let mut result = 0;
        SendMessageTimeoutW(hwnd, WM_CLOSE, 0, 0, SMTO_ABORTIFHUNG, 3000, &mut result);
        if process.is_null() {
            std::thread::sleep(std::time::Duration::from_millis(1000));
        } else {
            WaitForSingleObject(process, 5000);
            CloseHandle(process);
        }
        true
    }
}

fn notify_data(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid
}

/// Adds or updates the tray icon according to the current state.
fn update_icon() {
    TRAY.with(|t| {
        let mut t = t.borrow_mut();
        let Some(tray) = t.as_mut() else { return };
        if !tray.show_icon {
            return;
        }
        let (layer_index, layer) = platform::current_layer();
        let enabled = platform::is_enabled();
        let icon = if !enabled {
            tray.icons[2]
        } else if layer_index != 0 {
            tray.icons[1]
        } else {
            tray.icons[0]
        };
        let mut tip = format!("mouseless\nLayer: {layer}");
        if !enabled {
            tip = "mouseless (paused)".to_string();
        }
        if autostart::is_elevated() {
            tip.push_str("\nRunning as administrator");
        }

        let mut nid = notify_data(tray.hwnd);
        nid.uFlags = NIF_ICON | NIF_TIP | NIF_MESSAGE;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = icon;
        for (dst, src) in nid.szTip.iter_mut().zip(tip.encode_utf16().take(127)) {
            *dst = src;
        }
        unsafe {
            if tray.icon_added {
                if Shell_NotifyIconW(NIM_MODIFY, &nid) == 0 {
                    // the icon might have been removed, e.g. when explorer restarted
                    tray.icon_added = Shell_NotifyIconW(NIM_ADD, &nid) != 0;
                }
            } else {
                // this fails if the taskbar does not exist yet (e.g. directly after login), the icon is
                // added once it sends TaskbarCreated
                tray.icon_added = Shell_NotifyIconW(NIM_ADD, &nid) != 0;
                debug!("Tray icon added: {}", tray.icon_added);
            }
        }
    });
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            if matches!(lparam as u32, WM_LBUTTONUP | WM_RBUTTONUP) {
                show_menu(hwnd);
            }
            0
        }
        WM_STATE_CHANGED => {
            update_icon();
            0
        }
        WM_CLOSE => {
            quit();
            0
        }
        WM_QUERYENDSESSION => 1,
        WM_ENDSESSION => {
            if wparam != 0 {
                quit();
            }
            0
        }
        _ if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            TRAY.with(|t| {
                if let Some(tray) = t.borrow_mut().as_mut() {
                    tray.icon_added = false;
                }
            });
            update_icon();
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Ends the message loop. An open menu has to be closed first, since its own message loop would
/// otherwise keep running.
fn quit() {
    unsafe {
        EndMenu();
        PostQuitMessage(0);
    }
}

fn show_menu(hwnd: HWND) {
    let (_, layer) = platform::current_layer();
    let enabled = platform::is_enabled();
    let autostart = AUTOSTART_ENABLED.load(Ordering::Relaxed);
    let has_log = logger::log_file_path().is_some();
    let title = if enabled { format!("mouseless - layer: {layer}") } else { "mouseless - paused".to_string() };

    unsafe {
        let menu = CreatePopupMenu();
        let add = |flags: u32, id: usize, text: &str| {
            let text = wide(text);
            AppendMenuW(menu, flags, id, text.as_ptr());
        };
        let checked = |c: bool| if c { MF_CHECKED } else { MF_UNCHECKED };
        add(MF_STRING | MF_GRAYED, 0, &title);
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        add(MF_STRING | checked(enabled), CMD_ENABLED, "&Enabled");
        add(MF_STRING, CMD_RELOAD, "&Reload config");
        add(MF_STRING, CMD_EDIT_CONFIG, "Edit &config");
        add(MF_STRING | if has_log { 0 } else { MF_GRAYED }, CMD_OPEN_LOG, "Open &log");
        add(MF_STRING | checked(autostart), CMD_AUTOSTART, "&Start at login (as administrator)");
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        add(MF_STRING, CMD_EXIT, "E&xit");

        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        // required so that the menu closes when clicking somewhere else
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
            pt.x,
            pt.y,
            0,
            hwnd,
            std::ptr::null(),
        );
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);

        match cmd as usize {
            CMD_ENABLED => platform::set_enabled(!enabled),
            CMD_RELOAD => platform::request_reload(),
            CMD_EDIT_CONFIG => open_file(&platform::config_path(), true),
            CMD_OPEN_LOG => {
                if let Some(p) = logger::log_file_path() {
                    open_file(&p, true);
                }
            }
            CMD_AUTOSTART => toggle_autostart(!autostart),
            CMD_EXIT => PostQuitMessage(0),
            _ => {}
        }
    }
}

/// Opens the given file with its default program, or with notepad if there is none. This runs in the
/// background, since the main thread must never block (it handles the keyboard hook).
fn open_file(path: &Path, notepad_fallback: bool) {
    let path = path.to_path_buf();
    std::thread::spawn(move || unsafe {
        let file = wide(&path.to_string_lossy());
        let open = wide("open");
        let result = ShellExecuteW(
            std::ptr::null_mut(),
            open.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
        if result as isize <= 32 && notepad_fallback {
            let notepad = wide("notepad.exe");
            let params = wide(&format!("\"{}\"", path.display()));
            ShellExecuteW(
                std::ptr::null_mut(),
                open.as_ptr(),
                notepad.as_ptr(),
                params.as_ptr(),
                std::ptr::null(),
                SW_SHOWNORMAL,
            );
        }
    });
}

/// Queries the state of the autostart task in the background.
fn refresh_autostart_state() {
    std::thread::spawn(|| AUTOSTART_ENABLED.store(autostart::is_enabled(), Ordering::Relaxed));
}

fn toggle_autostart(enable: bool) {
    std::thread::spawn(move || {
        let result = if enable { autostart::enable() } else { autostart::disable() };
        AUTOSTART_ENABLED.store(autostart::is_enabled(), Ordering::Relaxed);
        match result {
            Ok(()) if enable => message_box(
                "mouseless will now start automatically with administrator rights when you log in.",
                false,
            ),
            Ok(()) => {}
            Err(e) => {
                warn!("Failed to change the autostart: {e}");
                message_box(&format!("Failed to change the autostart:\n{e}"), true);
            }
        }
    });
}

/// Shows a message box (blocks the calling thread until it is closed).
pub fn message_box(text: &str, is_error: bool) {
    let text = wide(text);
    let title = wide("mouseless");
    let icon = if is_error { MB_ICONERROR } else { MB_ICONINFORMATION };
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), title.as_ptr(), MB_OK | icon | MB_SETFOREGROUND) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
