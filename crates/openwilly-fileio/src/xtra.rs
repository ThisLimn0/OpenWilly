//! MOA Xtra interface for FILEIO.X32
//!
//! Implements the exact Macromedia Open Architecture (MOA) protocol as
//! reverse-engineered from the original FILEIO.X32 binary via Ghidra.
//!
//! ## Architecture
//! Director calls 5 exported functions:
//! - `DllGetClassObject` → stub, returns CLASS_E_CLASSNOTAVAILABLE
//! - `DllCanUnloadNow` → checks global refcount
//! - `DllGetClassForm` → registers classes with instance sizes + create/destroy callbacks
//! - `DllGetInterface` → registers interface factories, then calls the matching one
//! - `DllGetClassInfo` → allocates MoaClassInfoEntry array via IMoaCalloc
//!
//! Two classes per Xtra:
//! 1. **XtraEtc** (registration class) — vtable: QI/AddRef/Release/Register
//! 2. **ScriptXtra** (script class) — vtable: QI/AddRef/Release/CallHandler

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::debug_log;
use crate::fileops::FileIOInstance;

// ============================================================================
// GUIDs from original binary (16-byte values, little-endian)
// ============================================================================

/// ScriptXtra Class GUID: {AD27ED36-000A-ED0C-0000-0800076E489A}
/// This is the LARGER class (instSize=0x158) that handles Lingo script calls.
/// In the original, this is registered first (Class 1 = DAT_100102c0).
const CLASS1_GUID: [u8; 16] = [
    0x36, 0xED, 0x27, 0xAD, 0x0A, 0x00, 0x0C, 0xED,
    0x00, 0x00, 0x08, 0x00, 0x07, 0x6E, 0x48, 0x9A,
];

/// XtraEtc/Utility Class GUID: {ADC76C91-0043-FFA7-0000-0800076E489A}
/// This is the SMALLER class (instSize=0x20) that handles Xtra registration.
/// In the original, this is registered second (Class 2 = DAT_100102b0).
const CLASS2_GUID: [u8; 16] = [
    0x91, 0x6C, 0xC7, 0xAD, 0x43, 0x00, 0xA7, 0xFF,
    0x00, 0x00, 0x08, 0x00, 0x07, 0x6E, 0x48, 0x9A,
];

/// IID for XtraEtc interface: {AC3E7803-0002-8FE5-0000-0800-07160DC3}
const IID_XTRA_ETC: [u8; 16] = [
    0x03, 0x78, 0x3E, 0xAC, 0x02, 0x00, 0xE5, 0x8F,
    0x00, 0x00, 0x08, 0x00, 0x07, 0x16, 0x0D, 0xC3,
];

/// IID for IMoaMmXScript: {AC401FB6-0001-F9B0-0000-0800072C6326}
const IID_SCRIPT: [u8; 16] = [
    0xB6, 0x1F, 0x40, 0xAC, 0x01, 0x00, 0xB0, 0xF9,
    0x00, 0x00, 0x08, 0x00, 0x07, 0x2C, 0x63, 0x26,
];

// ============================================================================
// MOA Interface IIDs (from Ghidra analysis of original binary)
// These are QueryInterface'd in ScriptXtra Create and stored in instance data.
// ============================================================================

/// IID at DAT_10010070: queried → inst+0x18 (IMoaCalloc or similar)
const IID_QI_0: [u8; 16] = [
    0x21, 0x99, 0x9D, 0xAB, 0x02, 0x7F, 0x99, 0x2F,
    0x48, 0x7F, 0x08, 0x00, 0x07, 0x16, 0x0D, 0xC3,
];

/// IID at DAT_10010040: queried → inst+0x1C (IMoaCallback)
const IID_QI_1: [u8; 16] = [
    0x20, 0x8C, 0x69, 0x3A, 0xC3, 0x39, 0x1C, 0x10,
    0x9A, 0x9F, 0x00, 0x00, 0xC0, 0xDD, 0xAA, 0x4B,
];

/// IID at DAT_100100e0: queried → inst+0x20
const IID_QI_2: [u8; 16] = [
    0xB8, 0xE2, 0xC7, 0xAC, 0x0A, 0x00, 0xD0, 0xB4,
    0x00, 0x00, 0x08, 0x00, 0x07, 0x16, 0x0D, 0xC3,
];

/// IID for IMoaMmValue: {AC96CF89-0045-5879-0000-0040105023FB}
/// Queried → inst+0x24 — THE CORE INTERFACE for value conversion
const IID_MOA_MM_VALUE: [u8; 16] = [
    0x89, 0xCF, 0x96, 0xAC, 0x45, 0x00, 0x79, 0x58,
    0x00, 0x00, 0x00, 0x40, 0x10, 0x50, 0x23, 0xFB,
];

/// IID at DAT_10010170: queried → inst+0x28 (IMoaMmList or similar)
const IID_QI_4: [u8; 16] = [
    0x98, 0xCF, 0x96, 0xAC, 0x45, 0x00, 0x0B, 0x5C,
    0x00, 0x00, 0x00, 0x40, 0x10, 0x50, 0x23, 0xFB,
];

/// IID for IMoaMmUtils: {AC401B78-0000-FA7D-0000-0800072C6326}
/// Queried → inst+0x2C — used in CallHandler prolog
const IID_MOA_MM_UTILS: [u8; 16] = [
    0x78, 0x1B, 0x40, 0xAC, 0x00, 0x00, 0x7D, 0xFA,
    0x00, 0x00, 0x08, 0x00, 0x07, 0x2C, 0x63, 0x26,
];

/// IID at DAT_10010250: queried → inst+0x30
const IID_QI_6: [u8; 16] = [
    0xC0, 0xCF, 0x96, 0xAC, 0x45, 0x00, 0x97, 0x65,
    0x00, 0x00, 0x00, 0x40, 0x10, 0x50, 0x23, 0xFB,
];

/// IID for second Class1 (ScriptXtra) interface registration: {AC3E7803-...}
/// Same as IID_XTRA_ETC — in the original, ScriptXtra also exposes this
const IID_SCRIPT_ALT: [u8; 16] = [
    0x03, 0x78, 0x3E, 0xAC, 0x02, 0x00, 0xE5, 0x8F,
    0x00, 0x00, 0x08, 0x00, 0x07, 0x16, 0x0D, 0xC3,
];

/// IID for second Class2 (XtraEtc) interface registration: {AC734D52-...}
const IID_XTRA_ETC_ALT: [u8; 16] = [
    0x52, 0x4D, 0x73, 0xAC, 0x5D, 0x00, 0x2A, 0x04,
    0x00, 0x00, 0x08, 0x00, 0x07, 0x16, 0x0D, 0xC3,
];

// ============================================================================
// XTRA_INFO string (from original binary, used in Register callback)
// ============================================================================

const XTRA_INFO: &[u8] = b"\
-- xtra fileio -- CH May96 \n\
-- version 1.5.1 \n\
new object me \n\
fileName object me \n\
status object me \n\
error object me, int error \n\
setFilterMask me, string mask \n\
openFile object me, string fileName, int mode \n\
closeFile object me \n\
displayOpen object me \n\
displaySave object me, string title, string defaultFileName \n\
createFile object me, string fileName \n\
setPosition object me, int position \n\
getPosition object me \n\
getLength object me \n\
writeChar object me, string theChar \n\
writeString object me, string theString \n\
readChar object me \n\
readLine object me \n\
readFile object me \n\
readWord object me \n\
readToken object me, string skip, string break \n\
getFinderInfo object me \n\
setFinderInfo object me, string attributes \n\
delete object me \n\
+ version xtraRef \n\
* getOSDirectory \n\
\0";

const MSG_TABLE: &[u8] = b"msgTable\0";

// ============================================================================
// Global state
// ============================================================================

/// Global instance refcount — DllCanUnloadNow checks this
static GLOBAL_REFCOUNT: AtomicI32 = AtomicI32::new(0);

/// Version flags (matches original KEYPOLL: 0x2000180; FILEIO uses class index 1/2)
const VERSION_FLAGS: u32 = 0x0200_0180;

/// Class1 (ScriptXtra) instance size — large, holds file state
const CLASS1_INST_SIZE: u32 = 0x158;

/// Class2 (XtraEtc/Utility) instance size — small, registration only
const CLASS2_INST_SIZE: u32 = 0x20;

// ============================================================================
// GUIDs comparison helper
// ============================================================================

fn guid_eq(a: *const u8, b: &[u8; 16]) -> bool {
    if a.is_null() {
        return false;
    }
    unsafe {
        for i in 0..16 {
            if *a.add(i) != b[i] {
                return false;
            }
        }
    }
    true
}

// ============================================================================
// VTables (static, 4-entry each: QI, AddRef, Release, Method)
// ============================================================================

/// Wrapper to make *const c_void arrays Sync (they're function pointers, which are safe)
struct SyncVTable([*const c_void; 4]);
unsafe impl Sync for SyncVTable {}
impl SyncVTable {
    const fn as_ptr(&self) -> *const *const c_void {
        self.0.as_ptr()
    }
}

/// XtraEtc vtable — method slot is Register
static XTRA_ETC_VTABLE: SyncVTable = SyncVTable([
    xtra_etc_qi as *const c_void,
    xtra_etc_addref as *const c_void,
    xtra_etc_release as *const c_void,
    xtra_etc_register as *const c_void,
]);

/// ScriptXtra vtable — method slot is CallHandler
static SCRIPT_XTRA_VTABLE: SyncVTable = SyncVTable([
    script_xtra_qi as *const c_void,
    script_xtra_addref as *const c_void,
    script_xtra_release as *const c_void,
    script_xtra_call_handler as *const c_void,
]);

// ============================================================================
// Interface object layout: [vtable_ptr, parent_ptr, refcount]
// Size = 12 bytes (0x0C)
// ============================================================================

/// Interface object — allocated by factory via Director's IMoaCalloc
#[repr(C)]
struct InterfaceObj {
    vtable: *const *const c_void,
    parent: *mut c_void,
    refcount: i32,
}

// ============================================================================
// XtraEtc vtable methods
// ============================================================================

/// QueryInterface — delegates to parent's QI
unsafe extern "system" fn xtra_etc_qi(
    this: *mut InterfaceObj,
    iid: *const u8,
    ppv: *mut *mut c_void,
) -> i32 {
    if this.is_null() || ppv.is_null() {
        return 0x80004003_u32 as i32; // E_POINTER
    }
    // Delegate to parent's IMoaUnknown::QueryInterface (vtable[0])
    let parent = (*this).parent;
    if parent.is_null() {
        return 0x80004002_u32 as i32; // E_NOINTERFACE
    }
    let parent_vtable = *(parent as *const *const *const c_void);
    let qi_fn: unsafe extern "system" fn(*mut c_void, *const u8, *mut *mut c_void) -> i32 =
        std::mem::transmute(*parent_vtable);
    qi_fn(parent, iid, ppv)
}

/// AddRef
unsafe extern "system" fn xtra_etc_addref(this: *mut InterfaceObj) -> u32 {
    if this.is_null() {
        return 0;
    }
    (*this).refcount += 1;
    (*this).refcount as u32
}

/// Release
unsafe extern "system" fn xtra_etc_release(this: *mut InterfaceObj) -> u32 {
    if this.is_null() {
        return 0;
    }
    (*this).refcount -= 1;
    let rc = (*this).refcount;
    if rc <= 0 {
        // Release parent reference
        if !(*this).parent.is_null() {
            let parent = (*this).parent;
            let parent_vtable = *(parent as *const *const *const c_void);
            let release_fn: unsafe extern "system" fn(*mut c_void) -> u32 =
                std::mem::transmute(*parent_vtable.add(2));
            release_fn(parent);
        }
        GLOBAL_REFCOUNT.fetch_sub(1, Ordering::SeqCst);
        // Note: Director allocated the memory, Director will free it
        return 0;
    }
    rc as u32
}

/// Register — called by Director to learn what Lingo messages this Xtra handles
/// `this` is the interface object, first arg via vtable call is the receiver
/// The second param is IMoaRegister*
unsafe extern "system" fn xtra_etc_register(
    _this: *mut InterfaceObj,
    p_register: *mut c_void,
) -> i32 {
    debug_log("XtraEtc::Register called");

    if p_register.is_null() {
        return 0x80004003_u32 as i32;
    }

    // IMoaRegister vtable layout:
    //   [0] QI, [1] AddRef, [2] Release, [3..5] other, [6] SetInfo
    //   SetInfo is at offset 0x18 = vtable[6] (confirmed via Ghidra)
    // SetInfo(this, type, xtra_info_ptr, zero, msgTable_ptr)
    // type = 9 (for script xtras)
    let vtable = *(p_register as *const *const *const c_void);
    let set_info_fn: unsafe extern "system" fn(
        *mut c_void, // this (IMoaRegister*)
        i32,         // type = 9
        *const u8,   // xtra_info string
        i32,         // 0
        *const u8,   // "msgTable"
    ) -> i32 = std::mem::transmute(*vtable.add(6));

    let result = set_info_fn(
        p_register,
        9,
        XTRA_INFO.as_ptr(),
        0,
        MSG_TABLE.as_ptr(),
    );

    debug_log(&format!("XtraEtc::Register SetInfo returned 0x{:08X}", result as u32));
    result
}

// ============================================================================
// ScriptXtra vtable methods
// ============================================================================

/// QueryInterface — delegates to parent
unsafe extern "system" fn script_xtra_qi(
    this: *mut InterfaceObj,
    iid: *const u8,
    ppv: *mut *mut c_void,
) -> i32 {
    if this.is_null() || ppv.is_null() {
        return 0x80004003_u32 as i32;
    }
    let parent = (*this).parent;
    if parent.is_null() {
        return 0x80004002_u32 as i32;
    }
    let parent_vtable = *(parent as *const *const *const c_void);
    let qi_fn: unsafe extern "system" fn(*mut c_void, *const u8, *mut *mut c_void) -> i32 =
        std::mem::transmute(*parent_vtable);
    qi_fn(parent, iid, ppv)
}

/// AddRef
unsafe extern "system" fn script_xtra_addref(this: *mut InterfaceObj) -> u32 {
    if this.is_null() {
        return 0;
    }
    (*this).refcount += 1;
    (*this).refcount as u32
}

/// Release
unsafe extern "system" fn script_xtra_release(this: *mut InterfaceObj) -> u32 {
    if this.is_null() {
        return 0;
    }
    (*this).refcount -= 1;
    let rc = (*this).refcount;
    if rc <= 0 {
        if !(*this).parent.is_null() {
            let parent = (*this).parent;
            let parent_vtable = *(parent as *const *const *const c_void);
            let release_fn: unsafe extern "system" fn(*mut c_void) -> u32 =
                std::mem::transmute(*parent_vtable.add(2));
            release_fn(parent);
        }
        GLOBAL_REFCOUNT.fetch_sub(1, Ordering::SeqCst);
        return 0;
    }
    rc as u32
}

/// CallHandler - dispatches Lingo messages to FileIOInstance methods
///
/// call_info layout:
///   +0x08: handler_id (i32, 0-based)
///   +0x0C: return value slot (MoaMmValue, 8 bytes)
///   +0x18: pointer to argument array (each arg is 8-byte MoaMmValue)
///
/// IMoaMmValue (at inst+0x24) vtable methods used:
///   [13] ValueToInteger    offset 0x34
///   [17] ValueToStringPtr  offset 0x44
///   [19] StringReleasePtr  offset 0x4C
///   [22] IntegerToValue    offset 0x58
///   [25] StringToValue     offset 0x64
unsafe extern "system" fn script_xtra_call_handler(
    this: *mut InterfaceObj,
    call_info: *mut c_void,
) -> i32 {
    if this.is_null() || call_info.is_null() {
        return 0x80004005_u32 as i32;
    }

    let parent = (*this).parent;
    if parent.is_null() {
        return 0x80004005_u32 as i32;
    }

    let ci = call_info as *mut u8;
    let inst = parent as *mut u8;

    // Handler ID at call_info+0x08 (0-based)
    let msg_id = *(ci.add(8) as *const i32);

    // IMoaMmValue* from instance+0x24
    let imm_value = *(inst.add(0x24) as *const *mut c_void);

    debug_log(&format!("FILEIO CallHandler: msg_id={}", msg_id));

    // Ensure our FileIOInstance exists
    let _ = get_or_create_instance(parent);

    match msg_id {
        0 => handler_new(parent, imm_value, inst),
        1 => handler_return_string(parent, imm_value, ci, |i| i.file_name().to_string()),
        2 => handler_return_int(parent, imm_value, ci, |i| i.status()),
        3 => handler_error(parent, imm_value, ci),
        4 => handler_set_filter_mask(parent, imm_value, ci),
        5 => handler_open_file(parent, imm_value, ci),
        6 => handler_simple(parent, |i| i.close_file()),
        7 => handler_return_string(parent, imm_value, ci, |i| i.display_open()),
        8 => handler_display_save(parent, imm_value, ci),
        9 => handler_create_file(parent, imm_value, ci),
        10 => handler_set_position(parent, imm_value, ci),
        11 => handler_return_int(parent, imm_value, ci, |i| i.get_position()),
        12 => handler_return_int(parent, imm_value, ci, |i| i.get_length()),
        13 => handler_write_char(parent, imm_value, ci),
        14 => handler_write_string(parent, imm_value, ci),
        15 => handler_read_char(parent, imm_value, ci),
        16 => handler_return_string(parent, imm_value, ci, |i| i.read_line()),
        17 => handler_return_string(parent, imm_value, ci, |i| i.read_file()),
        18 => handler_return_string(parent, imm_value, ci, |i| i.read_word()),
        19 => handler_read_token(parent, imm_value, ci),
        20 => handler_return_int(parent, imm_value, ci, |i| i.get_finder_info()),
        21 => handler_set_finder_info(parent, imm_value, ci),
        22 => handler_simple(parent, |i| i.delete()),
        23 => handler_return_string(parent, imm_value, ci, |i| i.version()),
        24 => handler_return_string(parent, imm_value, ci, |i| i.get_os_directory()),
        _ => {
            debug_log(&format!("FILEIO: unknown handler {}", msg_id));
            0
        }
    }
}

// ============================================================================
// FileIO Instance Management
// ============================================================================

use std::collections::HashMap;
use std::sync::Mutex;

/// Map from parent pointer → FileIOInstance
static INSTANCE_MAP: std::sync::LazyLock<Mutex<HashMap<usize, Box<FileIOInstance>>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

/// Get or create a FileIOInstance for the given parent pointer
fn get_or_create_instance(parent: *mut c_void) -> &'static Mutex<HashMap<usize, Box<FileIOInstance>>> {
    let key = parent as usize;
    {
        let mut map = INSTANCE_MAP.lock().unwrap();
        if !map.contains_key(&key) {
            debug_log(&format!("Creating new FileIOInstance for parent {:p}", parent));
            map.insert(key, Box::new(FileIOInstance::new()));
        }
    }
    &INSTANCE_MAP
}

/// Remove a FileIOInstance
fn remove_instance(parent: *mut c_void) {
    let key = parent as usize;
    let mut map = INSTANCE_MAP.lock().unwrap();
    if map.remove(&key).is_some() {
        debug_log(&format!("Removed FileIOInstance for parent {:p}", parent));
    }
}

// ============================================================================
// MOA value helpers
// ============================================================================

/// Write an integer return value via IMoaMmValue::IntegerToValue (vtable[22])
unsafe fn moa_int_to_value(imm: *mut c_void, val: i32, out: *mut c_void) {
    if imm.is_null() { return; }
    let vt = *(imm as *const *const *const c_void);
    let f: unsafe extern "system" fn(*mut c_void, i32, *mut c_void) -> i32 =
        std::mem::transmute(*vt.add(22));
    f(imm, val, out);
}

/// Write a string return value via IMoaMmValue::StringToValue (vtable[25])
unsafe fn moa_str_to_value(imm: *mut c_void, s: &str, out: *mut c_void) {
    if imm.is_null() { return; }
    let cstr = format!("{}\0", s);
    let vt = *(imm as *const *const *const c_void);
    let f: unsafe extern "system" fn(*mut c_void, *const u8, *mut c_void) -> i32 =
        std::mem::transmute(*vt.add(25));
    f(imm, cstr.as_ptr(), out);
}

/// Read an integer argument via IMoaMmValue::ValueToInteger (vtable[13])
unsafe fn moa_value_to_int(imm: *mut c_void, val_ptr: *const c_void) -> i32 {
    if imm.is_null() { return 0; }
    let vt = *(imm as *const *const *const c_void);
    let f: unsafe extern "system" fn(*mut c_void, *const c_void, *mut i32) -> i32 =
        std::mem::transmute(*vt.add(13));
    let mut result: i32 = 0;
    f(imm, val_ptr, &mut result);
    result
}

/// Read a string argument via IMoaMmValue::ValueToStringPtr (vtable[17])
/// Returns the string content. Calls StringReleasePtr (vtable[19]) after.
unsafe fn moa_read_string(imm: *mut c_void, val_ptr: *const c_void) -> String {
    if imm.is_null() || val_ptr.is_null() { return String::new(); }
    let vt = *(imm as *const *const *const c_void);

    let mut out_ptr: *const u8 = ptr::null();
    let mut out_len: i32 = 0;
    let f: unsafe extern "system" fn(
        *mut c_void, *const c_void, *mut *const u8, *mut i32,
    ) -> i32 = std::mem::transmute(*vt.add(17));
    let hr = f(imm, val_ptr, &mut out_ptr, &mut out_len);

    if hr != 0 || out_ptr.is_null() {
        return String::new();
    }

    let result = if out_len > 0 {
        let slice = std::slice::from_raw_parts(out_ptr, out_len as usize);
        String::from_utf8_lossy(slice).into_owned()
    } else {
        let mut len = 0usize;
        while *out_ptr.add(len) != 0 && len < 4096 { len += 1; }
        let slice = std::slice::from_raw_parts(out_ptr, len);
        String::from_utf8_lossy(slice).into_owned()
    };

    // StringReleasePtr: vtable[19] — release the string pointer
    let release: unsafe extern "system" fn(*mut c_void, *const u8) -> i32 =
        std::mem::transmute(*vt.add(19));
    release(imm, out_ptr);

    result
}

// ============================================================================
// Individual handler implementations
// ============================================================================

/// Handler 0: new - initialize FileIO instance
unsafe fn handler_new(parent: *mut c_void, imm_value: *mut c_void, inst: *mut u8) -> i32 {
    debug_log("FILEIO handler 0 (new)");
    *(inst.add(0x50) as *mut i32) = 0; // status = OK

    // Set fileName to empty string
    if !imm_value.is_null() {
        moa_str_to_value(imm_value, "", inst.add(0x40) as *mut c_void);
    }

    // Set default filter mask
    let filter = b"All Files\0*.*\0\0";
    let dest = inst.add(0x54);
    ptr::write_bytes(dest, 0, 260);
    ptr::copy_nonoverlapping(filter.as_ptr(), dest, filter.len());

    // Create our FileIOInstance
    let key = parent as usize;
    let mut map = INSTANCE_MAP.lock().unwrap();
    map.insert(key, Box::new(FileIOInstance::new()));
    0
}

/// Generic: handler that returns a string via callback
unsafe fn handler_return_string(
    parent: *mut c_void, imm: *mut c_void, ci: *mut u8,
    f: fn(&mut FileIOInstance) -> String,
) -> i32 {
    if imm.is_null() { return 0; }
    let mut map = INSTANCE_MAP.lock().unwrap();
    if let Some(inst) = map.get_mut(&(parent as usize)) {
        let s = f(inst);
        moa_str_to_value(imm, &s, ci.add(0x0C) as *mut c_void);
    }
    0
}

/// Generic: handler that returns an integer via callback
unsafe fn handler_return_int(
    parent: *mut c_void, imm: *mut c_void, ci: *mut u8,
    f: fn(&mut FileIOInstance) -> i32,
) -> i32 {
    if imm.is_null() { return 0; }
    let mut map = INSTANCE_MAP.lock().unwrap();
    if let Some(inst) = map.get_mut(&(parent as usize)) {
        let v = f(inst);
        moa_int_to_value(imm, v, ci.add(0x0C) as *mut c_void);
    }
    0
}

/// Generic: handler that runs a simple action (no args, no return)
unsafe fn handler_simple(parent: *mut c_void, f: fn(&mut FileIOInstance)) -> i32 {
    let mut map = INSTANCE_MAP.lock().unwrap();
    if let Some(inst) = map.get_mut(&(parent as usize)) { f(inst); }
    0
}

/// Handler 3: error(errCode) - return error string
unsafe fn handler_error(parent: *mut c_void, imm: *mut c_void, ci: *mut u8) -> i32 {
    if imm.is_null() { return 0; }
    let args = *(ci.add(0x18) as *const *const u8);
    if args.is_null() { return 0; }
    let code = moa_value_to_int(imm, args as *const c_void);
    // Use FileIOInstance::error() method to resolve the error string
    let map = INSTANCE_MAP.lock().unwrap();
    let s = if let Some(inst) = map.get(&(parent as usize)) {
        inst.error(code)
    } else {
        crate::fileops::error_string(code)
    };
    moa_str_to_value(imm, s, ci.add(0x0C) as *mut c_void);
    0
}

/// Handler 4: setFilterMask(mask)
unsafe fn handler_set_filter_mask(parent: *mut c_void, imm: *mut c_void, ci: *mut u8) -> i32 {
    if imm.is_null() { return 0; }
    let args = *(ci.add(0x18) as *const *const u8);
    if args.is_null() { return 0; }
    let mask = moa_read_string(imm, args as *const c_void);
    let mut map = INSTANCE_MAP.lock().unwrap();
    if let Some(inst) = map.get_mut(&(parent as usize)) { inst.set_filter_mask(&mask); }
    0
}

/// Handler 5: openFile(fileName, mode)
unsafe fn handler_open_file(parent: *mut c_void, imm: *mut c_void, ci: *mut u8) -> i32 {
    if imm.is_null() { return 0; }
    let args = *(ci.add(0x18) as *const *const u8);
    if args.is_null() { return 0; }
    let name = moa_read_string(imm, args as *const c_void);
    let mode = moa_value_to_int(imm, args.add(8) as *const c_void);
    debug_log(&format!("openFile: '{}' mode={}", name, mode));
    let mut map = INSTANCE_MAP.lock().unwrap();
    if let Some(inst) = map.get_mut(&(parent as usize)) { inst.open_file(&name, mode); }
    0
}

/// Handler 8: displaySave(title, defaultFileName)
unsafe fn handler_display_save(parent: *mut c_void, imm: *mut c_void, ci: *mut u8) -> i32 {
    if imm.is_null() { return 0; }
    let args = *(ci.add(0x18) as *const *const u8);
    if args.is_null() { return 0; }
    let title = moa_read_string(imm, args as *const c_void);
    let default_name = moa_read_string(imm, args.add(8) as *const c_void);
    let mut map = INSTANCE_MAP.lock().unwrap();
    if let Some(inst) = map.get_mut(&(parent as usize)) {
        let r = inst.display_save(&title, &default_name);
        moa_str_to_value(imm, &r, ci.add(0x0C) as *mut c_void);
    }
    0
}

/// Handler 9: createFile(fileName)
unsafe fn handler_create_file(parent: *mut c_void, imm: *mut c_void, ci: *mut u8) -> i32 {
    if imm.is_null() { return 0; }
    let args = *(ci.add(0x18) as *const *const u8);
    if args.is_null() { return 0; }
    let name = moa_read_string(imm, args as *const c_void);
    let mut map = INSTANCE_MAP.lock().unwrap();
    if let Some(inst) = map.get_mut(&(parent as usize)) { inst.create_file(&name); }
    0
}

/// Handler 10: setPosition(pos)
unsafe fn handler_set_position(parent: *mut c_void, imm: *mut c_void, ci: *mut u8) -> i32 {
    if imm.is_null() { return 0; }
    let args = *(ci.add(0x18) as *const *const u8);
    if args.is_null() { return 0; }
    let pos = moa_value_to_int(imm, args as *const c_void);
    let mut map = INSTANCE_MAP.lock().unwrap();
    if let Some(inst) = map.get_mut(&(parent as usize)) { inst.set_position(pos); }
    0
}

/// Handler 13: writeChar(charStr)
unsafe fn handler_write_char(parent: *mut c_void, imm: *mut c_void, ci: *mut u8) -> i32 {
    if imm.is_null() { return 0; }
    let args = *(ci.add(0x18) as *const *const u8);
    if args.is_null() { return 0; }
    let s = moa_read_string(imm, args as *const c_void);
    let code = if s.is_empty() { 0 } else { s.as_bytes()[0] as i32 };
    let mut map = INSTANCE_MAP.lock().unwrap();
    if let Some(inst) = map.get_mut(&(parent as usize)) { inst.write_char(code); }
    0
}

/// Handler 14: writeString(str)
unsafe fn handler_write_string(parent: *mut c_void, imm: *mut c_void, ci: *mut u8) -> i32 {
    if imm.is_null() { return 0; }
    let args = *(ci.add(0x18) as *const *const u8);
    if args.is_null() { return 0; }
    let s = moa_read_string(imm, args as *const c_void);
    let mut map = INSTANCE_MAP.lock().unwrap();
    if let Some(inst) = map.get_mut(&(parent as usize)) { inst.write_string(&s); }
    0
}

/// Handler 15: readChar - return single char as string
unsafe fn handler_read_char(parent: *mut c_void, imm: *mut c_void, ci: *mut u8) -> i32 {
    if imm.is_null() { return 0; }
    let mut map = INSTANCE_MAP.lock().unwrap();
    if let Some(inst) = map.get_mut(&(parent as usize)) {
        let ch = inst.read_char();
        let s = if ch >= 0 { format!("{}", (ch as u8) as char) } else { String::new() };
        moa_str_to_value(imm, &s, ci.add(0x0C) as *mut c_void);
    }
    0
}

/// Handler 19: readToken(skip, break_chars)
unsafe fn handler_read_token(parent: *mut c_void, imm: *mut c_void, ci: *mut u8) -> i32 {
    if imm.is_null() { return 0; }
    let args = *(ci.add(0x18) as *const *const u8);
    if args.is_null() { return 0; }
    let skip = moa_read_string(imm, args as *const c_void);
    let brk = moa_read_string(imm, args.add(8) as *const c_void);
    let mut map = INSTANCE_MAP.lock().unwrap();
    if let Some(inst) = map.get_mut(&(parent as usize)) {
        let tok = inst.read_token(&skip, &brk);
        moa_str_to_value(imm, &tok, ci.add(0x0C) as *mut c_void);
    }
    0
}

/// Handler 21: setFinderInfo(attributes) — Mac only, stub
unsafe fn handler_set_finder_info(parent: *mut c_void, imm: *mut c_void, ci: *mut u8) -> i32 {
    if imm.is_null() { return 0; }
    let args = *(ci.add(0x18) as *const *const u8);
    if args.is_null() { return 0; }
    let attrs = moa_read_string(imm, args as *const c_void);
    let mut map = INSTANCE_MAP.lock().unwrap();
    if let Some(inst) = map.get_mut(&(parent as usize)) {
        inst.set_finder_info(&attrs);
    }
    0
}

// ============================================================================
// Class Create / Destroy callbacks (called by Director)
// ============================================================================

/// Class1 (ScriptXtra) create callback
/// Called by Director after allocating CLASS1_INST_SIZE (0x158) bytes.
/// Director pre-fills +0x00..+0x0C with vtable and pCallback.
/// We must NOT zero those! Only zero +0x18..end, then QI for 7 interfaces.
unsafe extern "C" fn class1_create(this: *mut c_void) -> i32 {
    debug_log("class1_create (ScriptXtra, 0x158 bytes)");
    if this.is_null() {
        return 0x80004005_u32 as i32;
    }
    let inst = this as *mut u8;

    // Zero only our portion: +0x18 .. +0x158 (preserve Director fields at +0x00..+0x17)
    ptr::write_bytes(inst.add(0x18), 0, CLASS1_INST_SIZE as usize - 0x18);

    // pCallback is at inst+0x08, use it for QueryInterface
    let p_callback = *(inst.add(0x08) as *const *mut c_void);
    if !p_callback.is_null() {
        let cb_vtable = *(p_callback as *const *const *const c_void);
        // QueryInterface = vtable[0]: fn(this, &iid, &mut out) -> i32
        let qi: unsafe extern "system" fn(*mut c_void, *const u8, *mut *mut c_void) -> i32 =
            std::mem::transmute(*cb_vtable.add(0));

        // QI for 7 interfaces into inst+0x18..+0x30 (4 bytes each)
        let iids: [&[u8; 16]; 7] = [
            &IID_QI_0, &IID_QI_1, &IID_QI_2,
            &IID_MOA_MM_VALUE, &IID_QI_4, &IID_MOA_MM_UTILS, &IID_QI_6,
        ];
        for (i, iid) in iids.iter().enumerate() {
            let slot = inst.add(0x18 + i * 4) as *mut *mut c_void;
            let hr = qi(p_callback, iid.as_ptr(), slot);
            if hr != 0 {
                debug_log(&format!("class1_create: QI[{}] failed hr=0x{:08X}", i, hr as u32));
            }
        }
    }

    debug_log(&format!(
        "class1_create: IMoaMmValue={:p}, IMoaMmUtils={:p}",
        *(inst.add(0x24) as *const *mut c_void),
        *(inst.add(0x2C) as *const *mut c_void),
    ));

    0 // S_OK
}

/// Class1 (ScriptXtra) destroy callback
/// Release all 7 QI'd interfaces at +0x18..+0x30
unsafe extern "C" fn class1_destroy(this: *mut c_void) -> i32 {
    debug_log("class1_destroy (ScriptXtra)");
    if !this.is_null() {
        let inst = this as *mut u8;
        // Release each QI'd interface (vtable[2] = Release)
        for i in 0..7 {
            let iface = *(inst.add(0x18 + i * 4) as *const *mut c_void);
            if !iface.is_null() {
                let vt = *(iface as *const *const *const c_void);
                let release: unsafe extern "system" fn(*mut c_void) -> u32 =
                    std::mem::transmute(*vt.add(2));
                release(iface);
            }
        }
    }
    // Clean up our FileIOInstance
    remove_instance(this);
    0
}

/// Class2 (XtraEtc/Utility) create callback
/// Called by Director after allocating CLASS2_INST_SIZE (0x20) bytes.
/// Preserve Director fields at +0x00..+0x0C, zero the rest.
unsafe extern "C" fn class2_create(this: *mut c_void) -> i32 {
    debug_log("class2_create (XtraEtc, 0x20 bytes)");
    if !this.is_null() {
        let inst = this as *mut u8;
        // Only zero +0x18..+0x20 (our portion), preserve Director fields
        ptr::write_bytes(inst.add(0x18), 0, CLASS2_INST_SIZE as usize - 0x18);
    }
    0
}

/// Class2 (XtraEtc/Utility) destroy callback
unsafe extern "C" fn class2_destroy(this: *mut c_void) -> i32 {
    debug_log("class2_destroy (XtraEtc)");
    let _ = this;
    0
}

// ============================================================================
// Interface factory functions (called from DllGetInterface callback)
// ============================================================================

/// Create XtraEtc interface object
/// Allocates 12 bytes via Director's IMoaCalloc, sets up vtable + parent ref
unsafe fn create_xtra_etc_interface(
    parent: *mut c_void,
    ppv: *mut *mut c_void,
    calloc: *mut c_void,
) -> i32 {
    debug_log("create_xtra_etc_interface");

    // Allocate 12 bytes via IMoaCalloc
    // IMoaCalloc vtable: [QI, AddRef, Release, NRAlloc, NRFree, ...]
    // NRAlloc is at vtable[3]: fn(this, size) -> *mut c_void
    let obj_ptr = moa_alloc(calloc, std::mem::size_of::<InterfaceObj>() as u32);
    if obj_ptr.is_null() {
        return 0x80040002_u32 as i32; // E_OUTOFMEMORY
    }

    let obj = obj_ptr as *mut InterfaceObj;
    (*obj).vtable = XTRA_ETC_VTABLE.as_ptr();
    (*obj).parent = parent;
    (*obj).refcount = 1;

    // AddRef parent twice (matching original behavior)
    if !parent.is_null() {
        let parent_vtable = *(parent as *const *const *const c_void);
        let addref_fn: unsafe extern "system" fn(*mut c_void) -> u32 =
            std::mem::transmute(*parent_vtable.add(1));
        addref_fn(parent);
        addref_fn(parent);
    }

    GLOBAL_REFCOUNT.fetch_add(1, Ordering::SeqCst);

    *ppv = obj_ptr;
    0 // S_OK
}

/// Create ScriptXtra interface object
unsafe fn create_script_xtra_interface(
    parent: *mut c_void,
    ppv: *mut *mut c_void,
    calloc: *mut c_void,
) -> i32 {
    debug_log("create_script_xtra_interface");

    let obj_ptr = moa_alloc(calloc, std::mem::size_of::<InterfaceObj>() as u32);
    if obj_ptr.is_null() {
        return 0x80040002_u32 as i32;
    }

    let obj = obj_ptr as *mut InterfaceObj;
    (*obj).vtable = SCRIPT_XTRA_VTABLE.as_ptr();
    (*obj).parent = parent;
    (*obj).refcount = 1;

    if !parent.is_null() {
        let parent_vtable = *(parent as *const *const *const c_void);
        let addref_fn: unsafe extern "system" fn(*mut c_void) -> u32 =
            std::mem::transmute(*parent_vtable.add(1));
        addref_fn(parent);
        addref_fn(parent);
    }

    GLOBAL_REFCOUNT.fetch_add(1, Ordering::SeqCst);

    *ppv = obj_ptr;
    0 // S_OK
}

/// Allocate memory via Director's IMoaCalloc
/// IMoaCalloc vtable[3] = NRAlloc(this, size) -> ptr
unsafe fn moa_alloc(calloc: *mut c_void, size: u32) -> *mut c_void {
    if calloc.is_null() {
        return ptr::null_mut();
    }
    let vtable = *(calloc as *const *const *const c_void);
    let nr_alloc: unsafe extern "system" fn(*mut c_void, u32) -> *mut c_void =
        std::mem::transmute(*vtable.add(3));
    nr_alloc(calloc, size)
}

// ============================================================================
// Central registration function
// (matches FUN_10001ae0 in original FILEIO.X32)
// ============================================================================

/// The central registration function that iterates over all classes.
///
/// Parameters:
/// - `filter`: optional GUID filter (if non-null, only register matching class)
/// - `class_cb`: callback for class registration
/// - `iface_cb`: callback for interface registration
/// - `user_data`: opaque data passed through to callbacks
///
/// The callbacks have different signatures depending on the caller:
/// - DllGetClassForm provides a class_cb that receives (guid, flags, size, create, destroy, userdata)
/// - DllGetInterface provides an iface_cb that receives (guid, flags, iid, flags, factory, userdata)
/// - DllGetClassInfo provides both callbacks
type ClassCallback = unsafe extern "C" fn(
    guid: *const u8,
    flags: u32,
    inst_size: u32,
    create_fn: *const c_void,
    destroy_fn: *const c_void,
    user_data: *mut c_void,
) -> i32;

type InterfaceCallback = unsafe extern "C" fn(
    class_guid: *const u8,
    class_flags: u32,
    iid: *const u8,
    iface_flags: u32,
    factory_fn: *const c_void,
    user_data: *mut c_void,
) -> i32;

unsafe fn register_classes(
    filter: *const u8,
    class_cb: Option<ClassCallback>,
    iface_cb: Option<InterfaceCallback>,
    user_data: *mut c_void,
) -> i32 {
    // --- Class 1: ScriptXtra (instSize=0x158, has CallHandler) ---
    if filter.is_null() || guid_eq(filter, &CLASS1_GUID) {
        if let Some(cb) = class_cb {
            let result = cb(
                CLASS1_GUID.as_ptr(),
                VERSION_FLAGS, // version/capability flags (original: 0x0200_0180)
                CLASS1_INST_SIZE,
                class1_create as *const c_void,
                class1_destroy as *const c_void,
                user_data,
            );
            if result != 0 {
                return result;
            }
        }
        if let Some(cb) = iface_cb {
            // Primary interface: IMoaMmXScript (CallHandler)
            let result = cb(
                CLASS1_GUID.as_ptr(),
                1,
                IID_SCRIPT.as_ptr(),
                1,
                create_script_xtra_interface_factory as *const c_void,
                user_data,
            );
            if result != 0 {
                return result;
            }
            // Alternate ScriptXtra interface (from original binary)
            let result = cb(
                CLASS1_GUID.as_ptr(),
                1,
                IID_SCRIPT_ALT.as_ptr(),
                1,
                create_script_xtra_interface_factory as *const c_void,
                user_data,
            );
            if result != 0 {
                return result;
            }
        }
    }

    // --- Class 2: XtraEtc/Utility (instSize=0x20, has Register) ---
    if filter.is_null() || guid_eq(filter, &CLASS2_GUID) {
        if let Some(cb) = class_cb {
            let result = cb(
                CLASS2_GUID.as_ptr(),
                VERSION_FLAGS, // version/capability flags (original: 0x0200_0180)
                CLASS2_INST_SIZE,
                class2_create as *const c_void,
                class2_destroy as *const c_void,
                user_data,
            );
            if result != 0 {
                return result;
            }
        }
        if let Some(cb) = iface_cb {
            // Primary interface: IMoaRegister-like (Register/SetInfo)
            let result = cb(
                CLASS2_GUID.as_ptr(),
                2,
                IID_XTRA_ETC.as_ptr(),
                2,
                create_xtra_etc_interface_factory as *const c_void,
                user_data,
            );
            if result != 0 {
                return result;
            }
            // Alternate XtraEtc interface (from original binary)
            let result = cb(
                CLASS2_GUID.as_ptr(),
                2,
                IID_XTRA_ETC_ALT.as_ptr(),
                2,
                create_xtra_etc_interface_factory as *const c_void,
                user_data,
            );
            if result != 0 {
                return result;
            }
        }
    }

    0
}

// ============================================================================
// Interface factory wrappers (called by Director via DllGetInterface)
//
// These match the signature Director expects:
//   factory(parent, ppInterface) -> HRESULT
//
// But we need IMoaCalloc to allocate. The original gets it from the parent
// object at offset +0x0C. We'll use the simpler approach of using Rust alloc
// since Director only cares about the vtable and parent pointer.
// ============================================================================

/// Factory for XtraEtc interface — called by Director via DllGetInterface callback
/// Signature: fn(parent: *mut c_void, ppv: *mut *mut c_void) -> i32
unsafe extern "C" fn create_xtra_etc_interface_factory(
    parent: *mut c_void,
    ppv: *mut *mut c_void,
) -> i32 {
    debug_log("create_xtra_etc_interface_factory");

    if ppv.is_null() {
        return 0x80004003_u32 as i32;
    }

    // Try to get IMoaCalloc from parent at offset +0x0C (Director convention)
    if !parent.is_null() {
        let calloc = *((parent as *const u8).add(0x0C) as *const *mut c_void);
        if !calloc.is_null() {
            debug_log("  -> using IMoaCalloc-based allocation");
            return create_xtra_etc_interface(parent, ppv, calloc);
        }
    }

    // Fallback: allocate using Rust's allocator (Box), then leak it
    debug_log("  -> using Rust Box allocation (no calloc available)");
    let obj = Box::new(InterfaceObj {
        vtable: XTRA_ETC_VTABLE.as_ptr(),
        parent,
        refcount: 1,
    });

    let obj_ptr = Box::into_raw(obj) as *mut c_void;

    // AddRef parent twice (matching original)
    if !parent.is_null() {
        let parent_vtable = *(parent as *const *const *const c_void);
        let addref_fn: unsafe extern "system" fn(*mut c_void) -> u32 =
            std::mem::transmute(*parent_vtable.add(1));
        addref_fn(parent);
        addref_fn(parent);
    }

    GLOBAL_REFCOUNT.fetch_add(1, Ordering::SeqCst);
    *ppv = obj_ptr;
    0
}

/// Factory for ScriptXtra interface
unsafe extern "C" fn create_script_xtra_interface_factory(
    parent: *mut c_void,
    ppv: *mut *mut c_void,
) -> i32 {
    debug_log("create_script_xtra_interface_factory");

    if ppv.is_null() {
        return 0x80004003_u32 as i32;
    }

    // Try to get IMoaCalloc from parent at offset +0x0C (Director convention)
    if !parent.is_null() {
        let calloc = *((parent as *const u8).add(0x0C) as *const *mut c_void);
        if !calloc.is_null() {
            debug_log("  -> using IMoaCalloc-based allocation");
            return create_script_xtra_interface(parent, ppv, calloc);
        }
    }

    // Fallback: allocate using Rust's allocator
    debug_log("  -> using Rust Box allocation (no calloc available)");
    let obj = Box::new(InterfaceObj {
        vtable: SCRIPT_XTRA_VTABLE.as_ptr(),
        parent,
        refcount: 1,
    });

    let obj_ptr = Box::into_raw(obj) as *mut c_void;

    if !parent.is_null() {
        let parent_vtable = *(parent as *const *const *const c_void);
        let addref_fn: unsafe extern "system" fn(*mut c_void) -> u32 =
            std::mem::transmute(*parent_vtable.add(1));
        addref_fn(parent);
        addref_fn(parent);
    }

    GLOBAL_REFCOUNT.fetch_add(1, Ordering::SeqCst);
    *ppv = obj_ptr;
    0
}

// ============================================================================
// DLL Exports — these are the 5 functions Director looks for
//
// CRITICAL: Must be `extern "C"` (NOT `extern "system"`) so that
// the export names are plain (e.g. "DllGetClassObject") without
// stdcall @N decoration.
// ============================================================================

/// Export 1: DllGetClassObject
/// Original: Always returns 0x80040005 (CLASS_E_CLASSNOTAVAILABLE)
/// Director does NOT use COM class factory — uses DllGetClassForm + DllGetInterface instead.
#[no_mangle]
pub unsafe extern "C" fn DllGetClassObject(
    _rclsid: *const u8,
    _riid: *const u8,
    _ppv: *mut *mut c_void,
) -> i32 {
    debug_log("DllGetClassObject called (stub → 0x80040005)");
    0x80040005_u32 as i32 // CLASS_E_CLASSNOTAVAILABLE
}

/// Export 2: DllCanUnloadNow
/// Returns S_OK (0) when no instances exist, else 0x80040003
#[no_mangle]
pub unsafe extern "C" fn DllCanUnloadNow() -> i32 {
    let rc = GLOBAL_REFCOUNT.load(Ordering::SeqCst);
    if rc == 0 {
        0 // S_OK — safe to unload
    } else {
        0x80040003_u32 as i32 // S_FALSE equivalent — still in use
    }
}

/// Export 3: DllGetClassForm
///
/// Director calls this to discover what classes the Xtra provides.
/// For each class, we report: GUID, instance size, create/destroy callbacks.
///
/// Original signature (from Ghidra):
///   DllGetClassForm(GUID *filter, instSize_out, createFn_out, destroyFn_out)
///
/// But actually the original uses the central registration function with a
/// local callback that populates OUT parameters. The actual export signature is:
///   DllGetClassForm(filter: *const GUID, p2, p3, p4) where p2-p4 are packed on stack
///
/// We replicate the original's approach using callbacks.
#[no_mangle]
pub unsafe extern "C" fn DllGetClassForm(
    filter: *const u8,
    out_inst_size: *mut u32,
    out_create_fn: *mut *const c_void,
    out_destroy_fn: *mut *const c_void,
) -> i32 {
    debug_log("DllGetClassForm called");

    // Use the registration function with a class callback that fills the OUT params
    #[repr(C)]
    struct FormUserData {
        out_inst_size: *mut u32,
        out_create_fn: *mut *const c_void,
        out_destroy_fn: *mut *const c_void,
        found: bool,
    }

    unsafe extern "C" fn form_class_cb(
        _guid: *const u8,
        _flags: u32,
        inst_size: u32,
        create_fn: *const c_void,
        destroy_fn: *const c_void,
        user_data: *mut c_void,
    ) -> i32 {
        let data = &mut *(user_data as *mut FormUserData);
        if !data.out_inst_size.is_null() {
            *data.out_inst_size = inst_size;
        }
        if !data.out_create_fn.is_null() {
            *data.out_create_fn = create_fn;
        }
        if !data.out_destroy_fn.is_null() {
            *data.out_destroy_fn = destroy_fn;
        }
        data.found = true;
        0
    }

    let mut data = FormUserData {
        out_inst_size,
        out_create_fn,
        out_destroy_fn,
        found: false,
    };

    register_classes(
        filter,
        Some(form_class_cb),
        None,
        &mut data as *mut FormUserData as *mut c_void,
    );

    if data.found {
        0 // S_OK
    } else {
        0x80040005_u32 as i32 // CLASS_E_CLASSNOTAVAILABLE
    }
}

/// Export 4: DllGetInterface
///
/// Director calls this after creating an instance to get an interface vtable.
/// It provides the parent object, class GUID, interface IID, and an OUT pointer.
///
/// We use the registration function with an interface callback that matches
/// the requested IID and calls the corresponding factory.
#[no_mangle]
pub unsafe extern "C" fn DllGetInterface(
    parent: *mut c_void,
    class_guid: *const u8,
    iid: *const u8,
    ppv: *mut *mut c_void,
) -> i32 {
    debug_log("DllGetInterface called");

    if ppv.is_null() {
        return 0x80004003_u32 as i32;
    }

    #[repr(C)]
    struct IfaceUserData {
        iid: *const u8,
        factory: Option<unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> i32>,
    }

    unsafe extern "C" fn iface_find_cb(
        _class_guid: *const u8,
        _class_flags: u32,
        cb_iid: *const u8,
        _iface_flags: u32,
        factory_fn: *const c_void,
        user_data: *mut c_void,
    ) -> i32 {
        let data = &mut *(user_data as *mut IfaceUserData);
        // Check if this interface's IID matches what we're looking for
        if guid_eq(data.iid, &*(cb_iid as *const [u8; 16])) {
            data.factory = Some(std::mem::transmute(factory_fn));
        }
        0
    }

    let mut data = IfaceUserData {
        iid,
        factory: None,
    };

    register_classes(
        class_guid,
        None,
        Some(iface_find_cb),
        &mut data as *mut IfaceUserData as *mut c_void,
    );

    match data.factory {
        Some(factory) => factory(parent, ppv),
        None => {
            debug_log("DllGetInterface: no matching interface found");
            *ppv = ptr::null_mut();
            0x80040004_u32 as i32 // E_NOINTERFACE
        }
    }
}

/// Export 5: DllGetClassInfo
///
/// Director calls this with an IMoaCalloc* to allocate a MoaClassInfoEntry array.
/// Each entry is 0x28 (40) bytes. We report 2 classes.
///
/// MoaClassInfoEntry layout (40 bytes each):
///   +0x00: GUID classID (16 bytes)
///   +0x10: GUID interfaceID (16 bytes)
///   +0x20: u32 flags1
///   +0x24: u32 flags2
#[no_mangle]
pub unsafe extern "C" fn DllGetClassInfo(
    calloc: *mut c_void,
    pp_class_info: *mut *mut c_void,
) -> i32 {
    debug_log("DllGetClassInfo called");

    if calloc.is_null() || pp_class_info.is_null() {
        return 0x80004003_u32 as i32;
    }

    // We have 2 classes, each with 1 interface = 2 entries + 1 terminator
    // Actually the original allocates enough for the entries and zeros them
    let num_entries = 2;
    let entry_size: u32 = 0x28; // 40 bytes per entry
    let total_size = entry_size * (num_entries + 1); // +1 for null terminator

    // Allocate via IMoaCalloc
    // IMoaCalloc vtable: [QI, AddRef, Release, NRAlloc, NRFree, ...]
    let vtable = *(calloc as *const *const *const c_void);
    let nr_alloc: unsafe extern "system" fn(*mut c_void, u32) -> *mut c_void =
        std::mem::transmute(*vtable.add(3));
    let buf = nr_alloc(calloc, total_size);

    if buf.is_null() {
        return 0x80040002_u32 as i32; // E_OUTOFMEMORY
    }

    // Zero-init all entries
    ptr::write_bytes(buf as *mut u8, 0, total_size as usize);

    // Entry 0: ScriptXtra class (CLASS1 = CallHandler)
    let entry0 = buf as *mut u8;
    ptr::copy_nonoverlapping(CLASS1_GUID.as_ptr(), entry0, 16);
    ptr::copy_nonoverlapping(IID_SCRIPT.as_ptr(), entry0.add(0x10), 16);

    // Entry 1: XtraEtc/Utility class (CLASS2 = Register)
    let entry1 = entry0.add(entry_size as usize);
    ptr::copy_nonoverlapping(CLASS2_GUID.as_ptr(), entry1, 16);
    ptr::copy_nonoverlapping(IID_XTRA_ETC.as_ptr(), entry1.add(0x10), 16);

    // Entry 2: terminator (all zeros, already done)

    *pp_class_info = buf;
    0
}
