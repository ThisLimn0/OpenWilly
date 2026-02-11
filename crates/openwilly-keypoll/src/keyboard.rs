//! Keyboard polling and hook functionality
//!
//! Implements the original KEYPOLL behavior:
//! - `bgOneKey`: GetAsyncKeyState check  
//! - `bgAllKeys`: scan all keys
//! - `hcKeysOff`: install WH_KEYBOARD hook that blocks ALL keyboard messages
//! - `hcKeysOn`: remove the keyboard hook

use std::ffi::c_void;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::ptr;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowsHookExA, UnhookWindowsHookEx, HHOOK, WH_KEYBOARD,
};

use crate::debug_log;

/// Global storage for the keyboard hook handle (null = no hook installed)
static HOOK_HANDLE: AtomicPtr<c_void> = AtomicPtr::new(ptr::null_mut());

/// The actual keyboard hook procedure, called by Windows.
/// Returns LRESULT(1) to block ALL keyboard messages from reaching the thread.
/// This matches the original MyKeyProc behavior exactly.
unsafe extern "system" fn keyboard_hook_proc(
    _code: i32,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> LRESULT {
    LRESULT(1)
}

/// Check if a specific key is currently pressed
///
/// Wraps GetAsyncKeyState — high bit set means key is down.
pub fn is_key_down(vkey: i32) -> bool {
    unsafe {
        let state = GetAsyncKeyState(vkey);
        (state as u16 & 0x8000) != 0
    }
}

/// Get a list of all currently pressed keys
///
/// Scans vKeys 4-255 (matches original: skips 0=none, 1=LBUTTON, 2=RBUTTON, 3=CANCEL).
pub fn get_all_keys_down() -> Vec<i32> {
    let mut keys = Vec::new();
    for vkey in 4..=255i32 {
        if is_key_down(vkey) {
            keys.push(vkey);
        }
    }
    keys
}

/// Install a thread-local keyboard hook that blocks ALL keyboard messages.
///
/// Original implementation (from Ghidra decompilation):
/// ```c
/// HOOKPROC hProc = (HOOKPROC)GetProcAddress(hModule, "MyKeyProc");
/// *(instance+0x28) = hProc;
/// *(instance+0x24) = SetWindowsHookExA(WH_KEYBOARD, hProc, NULL, GetCurrentThreadId());
/// ```
///
/// We use a proper `extern "system"` hook proc instead of the cdecl export hack.
pub fn keys_off() {
    debug_log("hcKeysOff: installing keyboard hook");

    // Don't install if already installed
    if !HOOK_HANDLE.load(Ordering::SeqCst).is_null() {
        debug_log("hcKeysOff: hook already installed");
        return;
    }

    unsafe {
        let thread_id = GetCurrentThreadId();
        match SetWindowsHookExA(
            WH_KEYBOARD,
            Some(keyboard_hook_proc),
            None,       // hmod = NULL for thread-local hook
            thread_id,
        ) {
            Ok(hhook) => {
                HOOK_HANDLE.store(hhook.0, Ordering::SeqCst);
                debug_log("hcKeysOff: keyboard hook installed successfully");
            }
            Err(e) => {
                debug_log(&format!("hcKeysOff: SetWindowsHookExA failed: {:?}", e));
            }
        }
    }
}

/// Remove the keyboard hook, allowing keyboard messages through again.
///
/// Original implementation:
/// ```c
/// UnhookWindowsHookEx(*(HHOOK*)(instance + 0x24));
/// *(instance + 0x20) = 1;  // hookEnabled = ready
/// ```
pub fn keys_on() {
    debug_log("hcKeysOn: removing keyboard hook");

    let h = HOOK_HANDLE.swap(ptr::null_mut(), Ordering::SeqCst);
    if !h.is_null() {
        unsafe {
            let _ = UnhookWindowsHookEx(HHOOK(h));
            debug_log("hcKeysOn: keyboard hook removed");
        }
    } else {
        debug_log("hcKeysOn: no hook was installed");
    }
}

/// Check if keyboard hook is currently installed
pub fn is_hook_active() -> bool {
    !HOOK_HANDLE.load(Ordering::SeqCst).is_null()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_not_active_initially() {
        assert!(!is_hook_active());
    }
}
