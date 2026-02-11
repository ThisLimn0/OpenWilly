//! DirectDraw wrapper DLL
//!
//! This DLL acts as a drop-in replacement for ddraw.dll, intercepting
//! DirectDraw calls and translating them to modern graphics APIs.
//!
//! The game will load this DLL instead of the system ddraw.dll, allowing
//! us to provide compatibility with modern Windows and GPUs.

use std::ffi::c_void;
use windows::core::*;

/// DLL entry point
#[no_mangle]
#[allow(non_snake_case)]
pub extern "system" fn DllMain(
    _hinst_dll: isize,
    fdw_reason: u32,
    _lpv_reserved: *mut c_void,
) -> i32 {
    const DLL_PROCESS_ATTACH: u32 = 1;
    const DLL_PROCESS_DETACH: u32 = 0;

    match fdw_reason {
        DLL_PROCESS_ATTACH => {
            // Initialize logging when DLL is loaded
            let _ = tracing_subscriber::fmt()
                .with_target(false)
                .with_file(true)
                .with_line_number(true)
                .try_init();
            
            tracing::info!("OpenWilly DirectDraw wrapper loaded");
        }
        DLL_PROCESS_DETACH => {
            tracing::info!("OpenWilly DirectDraw wrapper unloaded");
        }
        _ => {}
    }

    1 // TRUE
}

/// DirectDrawCreate - Main entry point for DirectDraw
///
/// This is the function games call to create a DirectDraw interface.
/// We intercept it and return our own implementation.
#[no_mangle]
pub unsafe extern "system" fn DirectDrawCreate(
    _lpguid: *mut windows::core::GUID,
    _lplpdd: *mut *mut c_void,
    _punkouter: *mut c_void,
) -> HRESULT {
    tracing::info!("DirectDrawCreate called");
    
    // TODO: Create our DirectDraw wrapper object
    // For now, return E_NOTIMPL
    
    HRESULT(-1) // E_NOTIMPL placeholder
}

/// DirectDrawCreateEx - Extended DirectDraw creation
#[no_mangle]
pub unsafe extern "system" fn DirectDrawCreateEx(
    _lpguid: *mut windows::core::GUID,
    _lplpdd: *mut *mut c_void,
    _iid: *const windows::core::GUID,
    _punkouter: *mut c_void,
) -> HRESULT {
    tracing::info!("DirectDrawCreateEx called");
    
    // TODO: Implement
    HRESULT(-1) // E_NOTIMPL placeholder
}

/// DirectDrawEnumerateA - Enumerate DirectDraw devices
#[no_mangle]
pub unsafe extern "system" fn DirectDrawEnumerateA(
    _lpcallback: isize,
    _lpcontext: *mut c_void,
) -> HRESULT {
    tracing::info!("DirectDrawEnumerateA called");
    
    // TODO: Enumerate our virtual device
    HRESULT(0) // S_OK placeholder
}

/// DirectDrawEnumerateW - Enumerate DirectDraw devices (Wide)
#[no_mangle]
pub unsafe extern "system" fn DirectDrawEnumerateW(
    _lpcallback: isize,
    _lpcontext: *mut c_void,
) -> HRESULT {
    tracing::info!("DirectDrawEnumerateW called");
    
    // TODO: Enumerate our virtual device
    HRESULT(0) // S_OK placeholder
}

// TODO: Implement IDirectDraw interface and all its methods
// This is a massive undertaking and will be done incrementally:
//
// IDirectDraw methods:
// - QueryInterface, AddRef, Release (COM basics)
// - CreateSurface
// - CreatePalette
// - SetDisplayMode
// - SetCooperativeLevel
// - WaitForVerticalBlank
// - GetDisplayMode
// - etc.

#[cfg(test)]
mod tests {

    #[test]
    fn test_placeholder() {
        // TODO: Add tests
    }
}
