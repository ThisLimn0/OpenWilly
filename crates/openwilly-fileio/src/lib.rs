//! OpenWilly FILEIO.X32 Replacement
//!
//! Drop-in replacement for the FileIO Xtra used by Macromedia Director 6 games.
//! Provides file I/O operations with integrated CD-path bypass.
//!
//! ## Original Lingo Interface
//! ```text
//! xtra fileio -- CH May96
//! new object me -- create a new child instance
//! fileName object me -- return fileName string of the open file
//! status object me -- return the error code of the last method called
//! error object me, int error -- return the error string of the error
//! setFilterMask me, string mask -- set the filter mask for dialogs
//! openFile object me, string fileName, int mode -- opens named file (0=r/w 1=r 2=w)
//! closeFile object me -- close the file
//! displayOpen object me -- displays an open dialog
//! displaySave object me, string title, string defaultFileName -- displays save dialog
//! createFile object me, string fileName -- creates a new file
//! setPosition object me, int position -- set the file position
//! getPosition object me -- get the file position
//! getLength object me -- get the length of the open file
//! writeChar object me, string theChar -- write a single character
//! writeString object me, string theString -- write a null-terminated string
//! readChar object me -- read the next character
//! readLine object me -- read the next line
//! readFile object me -- read from current position to EOF
//! readWord object me -- read the next word
//! readToken object me, string skip, string break -- read the next token
//! getFinderInfo object me -- Mac only (stub)
//! setFinderInfo object me, string attributes -- Mac only (stub)
//! delete object me -- deletes the open file
//! + version xtraRef -- display version info
//! * getOSDirectory -- returns the Windows directory path
//! ```
//!
//! ## Build Instructions
//! ```bash
//! rustup target add i686-pc-windows-msvc
//! cargo build --release --target i686-pc-windows-msvc -p openwilly-fileio
//! copy target\i686-pc-windows-msvc\release\openwilly_fileio.dll game\Xtras\FILEIO.X32
//! ```

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

mod xtra;
mod fileops;
mod pathmap;

use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

/// Global debug flag
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
            if std::env::var("OPENWILLY_DEBUG").is_ok() {
                DEBUG_LOG.store(true, Ordering::SeqCst);
                debug_log("OpenWilly FILEIO.X32 loaded (debug mode)");
            }
            // Initialize path mapping from environment
            pathmap::init();
        }
        DLL_PROCESS_DETACH => {
            debug_log("OpenWilly FILEIO.X32 unloaded");
        }
        _ => {}
    }
    1
}

/// Debug logging helper – outputs to OutputDebugString
pub(crate) fn debug_log(msg: &str) {
    if DEBUG_LOG.load(Ordering::Relaxed) {
        #[cfg(target_os = "windows")]
        unsafe {
            use windows::core::PCSTR;
            use windows::Win32::System::Diagnostics::Debug::OutputDebugStringA;
            let msg_with_newline = format!("[FILEIO] {}\0", msg);
            OutputDebugStringA(PCSTR::from_raw(msg_with_newline.as_ptr()));
        }
    }
}

// Re-export Xtra DLL entries
pub use xtra::*;
