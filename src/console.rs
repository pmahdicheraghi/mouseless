//! mouseless is a GUI application (so that no console window appears when it runs in the background).
//! When it is started from a terminal, it attaches to the console of the terminal to print its output.

use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE, HANDLE, INVALID_HANDLE_VALUE};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, GetFileType, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_CHAR, FILE_TYPE_UNKNOWN, OPEN_EXISTING,
};
use windows_sys::Win32::System::Console::{
    AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_HANDLE, STD_OUTPUT_HANDLE,
};

/// Makes stdout and stderr usable if possible: either they are redirected (e.g. to a file or pipe), or
/// the process is attached to the console of its parent. Returns whether output is possible.
pub fn attach() -> bool {
    unsafe {
        let redirected = |h: STD_HANDLE| {
            let handle = GetStdHandle(h);
            is_valid(handle) && !matches!(GetFileType(handle), FILE_TYPE_CHAR | FILE_TYPE_UNKNOWN)
        };
        let out_redirected = redirected(STD_OUTPUT_HANDLE);
        let err_redirected = redirected(STD_ERROR_HANDLE);
        if out_redirected && err_redirected {
            return true;
        }
        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            return out_redirected;
        }
        let conout: Vec<u16> = "CONOUT$".encode_utf16().chain(std::iter::once(0)).collect();
        let handle = CreateFileW(
            conout.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if !is_valid(handle) {
            return out_redirected;
        }
        if !out_redirected {
            SetStdHandle(STD_OUTPUT_HANDLE, handle);
        }
        if !err_redirected {
            SetStdHandle(STD_ERROR_HANDLE, handle);
        }
        // the prompt of the terminal has already been printed, start on a new line
        if !out_redirected {
            println!();
        }
        true
    }
}

fn is_valid(h: HANDLE) -> bool {
    !h.is_null() && h != INVALID_HANDLE_VALUE
}
