//! OpenWilly KEYPOLL.X32 Replacement
//!
//! This is a drop-in replacement for the KeyPoll Xtra used by Director 6.0 games.
//! It provides keyboard polling functionality with optional filtering of
//! unwanted keys (like ESC during text input).
//!
//! ## Original Lingo Interface
//! ```lingo
//! xtra KeyPoll
//! * bgOneKey integer keyCode -- returns TRUE if key (argument) is down, else FALSE
//! * bgAllKeys -- returns a linear list of the keycodes of every key currently down
//! * hcKeysOff -- prevents Director from receiving keyboard messages
//! * hcKeysOn  -- enables Director to receive keyboard messages
//! ```
//!
//! ## Build Instructions
//! ```bash
//! # Install 32-bit target
//! rustup target add i686-pc-windows-msvc
//! 
//! # Build 32-bit DLL
//! cargo build --release --target i686-pc-windows-msvc -p openwilly-keypoll
//! 
//! # Copy to game directory
//! copy target\i686-pc-windows-msvc\release\openwilly_keypoll.dll game\Xtras\KEYPOLL.X32
//! ```

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

mod xtra;
mod keyboard;

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

/// Global flag for debug logging
static DEBUG_LOG: AtomicBool = AtomicBool::new(false);

/// DLL Entry Point
#[no_mangle]
pub extern "system" fn DllMain(
    _hinst_dll: isize,
    fdw_reason: u32,
    _lpv_reserved: *mut c_void,
) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    const DLL_PROCESS_DETACH: u32 = 0;

    match fdw_reason {
        DLL_PROCESS_ATTACH => {
            // Check for debug environment variable
            if std::env::var("OPENWILLY_DEBUG").is_ok() {
                DEBUG_LOG.store(true, Ordering::SeqCst);
                debug_log("OpenWilly KEYPOLL.X32 loaded (debug mode)");
            }
        }
        DLL_PROCESS_DETACH => {
            if DEBUG_LOG.load(Ordering::SeqCst) {
                debug_log("OpenWilly KEYPOLL.X32 unloaded");
            }
        }
        _ => {}
    }

    1 // TRUE
}

/// Debug logging helper
fn debug_log(msg: &str) {
    if DEBUG_LOG.load(Ordering::Relaxed) {
        // Write to OutputDebugString for viewing in debuggers
        #[cfg(target_os = "windows")]
        unsafe {
            use windows::core::PCSTR;
            use windows::Win32::System::Diagnostics::Debug::OutputDebugStringA;
            
            let msg_with_newline = format!("[KEYPOLL] {}\0", msg);
            OutputDebugStringA(PCSTR::from_raw(msg_with_newline.as_ptr()));
        }
    }
}

// Re-export the Xtra interface functions
pub use xtra::*;
