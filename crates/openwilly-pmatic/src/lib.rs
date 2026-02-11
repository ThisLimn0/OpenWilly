//! OpenWilly PMATIC.X32 Replacement (No-op Stub)
//!
//! PrintOMatic Xtra stub — the game does not use any PrintOMatic functionality.
//! This DLL exists only so Director doesn't show an error about a missing Xtra.
//!
//! Exports the standard 5 MOA entry points with correct `extern "C"` linkage:
//! - DllGetClassObject → CLASS_E_CLASSNOTAVAILABLE
//! - DllCanUnloadNow → S_OK
//! - DllGetClassForm → CLASS_E_CLASSNOTAVAILABLE (no classes)
//! - DllGetInterface → E_NOINTERFACE
//! - DllGetClassInfo → S_OK (empty array)

#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use std::ffi::c_void;
use std::ptr;

// ============================================================================
// DLL Entry Point
// ============================================================================

#[no_mangle]
pub extern "system" fn DllMain(
    _hinst_dll: isize,
    _fdw_reason: u32,
    _lpv_reserved: *mut c_void,
) -> i32 {
    1 // TRUE
}

// ============================================================================
// DLL Exports — extern "C" to avoid @N decoration
// ============================================================================

/// DllGetClassObject — stub
#[no_mangle]
pub unsafe extern "C" fn DllGetClassObject(
    _rclsid: *const u8,
    _riid: *const u8,
    _ppv: *mut *mut c_void,
) -> i32 {
    0x80040005_u32 as i32 // CLASS_E_CLASSNOTAVAILABLE
}

/// DllCanUnloadNow — always OK (no instances)
#[no_mangle]
pub unsafe extern "C" fn DllCanUnloadNow() -> i32 {
    0 // S_OK
}

/// DllGetClassForm — no classes to register
#[no_mangle]
pub unsafe extern "C" fn DllGetClassForm(
    _filter: *const u8,
    _out_inst_size: *mut u32,
    _out_create_fn: *mut *const c_void,
    _out_destroy_fn: *mut *const c_void,
) -> i32 {
    0x80040005_u32 as i32 // CLASS_E_CLASSNOTAVAILABLE
}

/// DllGetInterface — no interfaces
#[no_mangle]
pub unsafe extern "C" fn DllGetInterface(
    _parent: *mut c_void,
    _class_guid: *const u8,
    _iid: *const u8,
    ppv: *mut *mut c_void,
) -> i32 {
    if !ppv.is_null() {
        unsafe { *ppv = ptr::null_mut(); }
    }
    0x80040004_u32 as i32 // E_NOINTERFACE
}

/// DllGetClassInfo — return empty class info
#[no_mangle]
pub unsafe extern "C" fn DllGetClassInfo(
    calloc: *mut c_void,
    pp_class_info: *mut *mut c_void,
) -> i32 {
    if calloc.is_null() || pp_class_info.is_null() {
        return 0x80004003_u32 as i32;
    }

    // Allocate a single zeroed entry (terminator) via IMoaCalloc
    let vtable = *(calloc as *const *const *const c_void);
    let nr_alloc: unsafe extern "system" fn(*mut c_void, u32) -> *mut c_void =
        std::mem::transmute(*vtable.add(3));
    let buf = nr_alloc(calloc, 0x28); // one empty entry
    if buf.is_null() {
        return 0x80040002_u32 as i32;
    }
    ptr::write_bytes(buf as *mut u8, 0, 0x28);
    *pp_class_info = buf;
    0
}
