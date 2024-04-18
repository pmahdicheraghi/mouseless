extern crate device_query;
extern crate mouse_rs;
extern crate winapi;

use device_query::{DeviceEvents, DeviceState, Keycode};
use mouse_rs::Mouse;
use std::ptr;
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::Duration;
use winapi::shared::minwindef::{DWORD, HINSTANCE, LPARAM, LRESULT, WPARAM};
use winapi::shared::ntdef::NULL;
use winapi::um::libloaderapi::GetModuleHandleW;
use winapi::um::winuser::{
    CallNextHookEx, SetWindowsHookExA, UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, VK_SPACE,
    WH_KEYBOARD_LL,
};
use winapi::um::winuser::{WM_KEYDOWN, WM_KEYUP};

fn main() {
    let device_state = DeviceState::new();

    let up = Arc::new(RwLock::new(false));
    let down = Arc::new(RwLock::new(false));
    let right = Arc::new(RwLock::new(false));
    let left = Arc::new(RwLock::new(false));
    let active = Arc::new(RwLock::new(false));

    let up_keydown = Arc::clone(&up);
    let down_keydown = Arc::clone(&down);
    let left_keydown = Arc::clone(&left);
    let right_keydown = Arc::clone(&right);
    let active_keydown = Arc::clone(&active);

    let up_keyup = Arc::clone(&up);
    let down_keyup = Arc::clone(&down);
    let left_keyup = Arc::clone(&left);
    let right_keyup = Arc::clone(&right);
    let active_keyup = Arc::clone(&active);

    // Set up hook to intercept keyboard events
    unsafe {
        let hinstance = GetModuleHandleW(NULL as *const u16);
        let hook = SetWindowsHookExA(
            WH_KEYBOARD_LL,
            Some(keyboard_proc),
            hinstance as HINSTANCE,
            0,
        );
        if hook.is_null() {
            panic!("Failed to set hook");
        }

        // Keep the program running to maintain the hook
        std::thread::park();

        UnhookWindowsHookEx(hook);
    }

    let _guard_keydown = device_state.on_key_down(move |key| {
        if *key == Keycode::Space {
            *active_keydown.write().unwrap() = true;
        } else if *key == Keycode::W {
            *up_keydown.write().unwrap() = true;
        } else if *key == Keycode::S {
            *down_keydown.write().unwrap() = true;
        } else if *key == Keycode::A {
            *left_keydown.write().unwrap() = true;
        } else if *key == Keycode::D {
            *right_keydown.write().unwrap() = true;
        }
        println!("Down: {:#?}", key);
    });

    let _guard_keyup = device_state.on_key_up(move |key| {
        if *key == Keycode::Space {
            *active_keyup.write().unwrap() = false;
        } else if *key == Keycode::W {
            *up_keyup.write().unwrap() = false;
        } else if *key == Keycode::S {
            *down_keyup.write().unwrap() = false;
        } else if *key == Keycode::A {
            *left_keyup.write().unwrap() = false;
        } else if *key == Keycode::D {
            *right_keyup.write().unwrap() = false;
        }
        println!("Up: {:#?}", key);
    });

    loop {
        let mouse = Mouse::new();
        let pos = mouse.get_position().unwrap();
        if *active.read().unwrap() {
            mouse
                .move_to(
                    pos.x + if *right.read().unwrap() { 5 } else { 0 }
                        - if *left.read().unwrap() { 5 } else { 0 },
                    pos.y + if *down.read().unwrap() { 5 } else { 0 }
                        - if *up.read().unwrap() { 5 } else { 0 },
                )
                .unwrap();
        }
        thread::sleep(Duration::from_millis(1)); // Corrected the sleep duration to milliseconds
    }
}

unsafe extern "system" fn keyboard_proc(code: i32, wParam: WPARAM, lParam: LPARAM) -> LRESULT {
    if code >= 0 && (wParam == WM_KEYDOWN as WPARAM || wParam == WM_KEYUP as WPARAM) {
        let kbd = *(lParam as *const KBDLLHOOKSTRUCT);
        if kbd.vkCode == VK_SPACE as u32 {
            // Handle space key here, for example, block further processing by returning -1
            return -1;
        }
    }
    // Call the next hook in the hook chain
    CallNextHookEx(ptr::null_mut(), code, wParam, lParam)
}
