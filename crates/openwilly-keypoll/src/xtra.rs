//! MOA Xtra interface for KEYPOLL.X32
//!
//! Implements the exact Macromedia Open Architecture (MOA) protocol as
//! reverse-engineered from the original KEYPOLL.X32 binary via Ghidra.
//!
//! ## Architecture
//! Same two-class pattern as FILEIO:
//! 1. **XtraEtc** (GUID {70A5D63A-...}) — registration, instSize=0x18 (24 bytes)
//! 2. **ScriptXtra** (GUID {1B2C229E-...}) — script handlers, instSize=0x2C (44 bytes)
//!
//! Plus one extra export: `MyKeyProc` (keyboard hook stub, returns 1)

use std::ffi::c_void;
use std::ptr;
use std::sync::atomic::{AtomicI32, Ordering};

use crate::debug_log;
use crate::keyboard;

// ============================================================================
// GUIDs from original binary (raw 16-byte little-endian)
// ============================================================================

/// XtraEtc Class GUID: {70A5D63A-3892-11D0-9E3B-00050270B208}
const CLASS1_GUID: [u8; 16] = [
    0x3A, 0xD6, 0xA5, 0x70, 0x92, 0x38, 0xD0, 0x11,
    0x9E, 0x3B, 0x00, 0x05, 0x02, 0x70, 0xB2, 0x08,
];

/// ScriptXtra Class GUID: {1B2C229E-3893-11D0-9E3B-00050270B208}
const CLASS2_GUID: [u8; 16] = [
    0x9E, 0x22, 0x2C, 0x1B, 0x93, 0x38, 0xD0, 0x11,
    0x9E, 0x3B, 0x00, 0x05, 0x02, 0x70, 0xB2, 0x08,
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

/// IID for IMoaMmValue: {AC96CF89-0045-5879-0000-0040105023FB}
const IID_MOA_MM_VALUE: [u8; 16] = [
    0x89, 0xCF, 0x96, 0xAC, 0x45, 0x00, 0x79, 0x58,
    0x00, 0x00, 0x00, 0x40, 0x10, 0x50, 0x23, 0xFB,
];

/// IID for IMoaMmList: {AC96CF98-0045-5C0B-0000-0040105023FB}
const IID_MOA_MM_LIST: [u8; 16] = [
    0x98, 0xCF, 0x96, 0xAC, 0x45, 0x00, 0x0B, 0x5C,
    0x00, 0x00, 0x00, 0x40, 0x10, 0x50, 0x23, 0xFB,
];

// ============================================================================
// XTRA_INFO string (from original binary)
// ============================================================================

const XTRA_INFO: &[u8] = b"\
-- KeyPoll Xtra v2.0d3\n\
-- by Brian Gray\n\
-- (c) 1996 Macromedia, Inc.  All Rights Reserved.\n\
-- http://www.macromedia.com\n\
\n\
-- hcKeysOff and hcKeysOn by Brian Sharon\n\
-- (c) 1997 Human Code, Inc.  All Rights Reserved.\n\
-- http://www.humancode.com\n\
--\n\
xtra KeyPoll\n\
new object me\n\
\n\
-- KeyPoll handlers --\n\
* bgOneKey integer keyCode -- returns TRUE if key (argument) is down, else FALSE\n\
* bgAllKeys -- returns a linear list of the keycodes of every key currently down\n\
* hcKeysOff -- prevents Director from receiving keyboard messages\n\
* hcKeysOn  -- enables Director to receive keyboard messages\n\
\0";

const MSG_TABLE: &[u8] = b"msgTable\0";

// ============================================================================
// Global state
// ============================================================================

static GLOBAL_REFCOUNT: AtomicI32 = AtomicI32::new(0);

const VERSION_FLAGS: u32 = 0x0200_0180;

/// Class1 (XtraEtc) instance size: 0x18 = 24 bytes
const CLASS1_INST_SIZE: u32 = 0x18;

/// Class2 (ScriptXtra) instance size: 0x2C = 44 bytes
const CLASS2_INST_SIZE: u32 = 0x2C;

// ============================================================================
// GUID comparison helper
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
// VTables
// ============================================================================

struct SyncVTable([*const c_void; 4]);
unsafe impl Sync for SyncVTable {}
impl SyncVTable {
    const fn as_ptr(&self) -> *const *const c_void {
        self.0.as_ptr()
    }
}

static XTRA_ETC_VTABLE: SyncVTable = SyncVTable([
    xtra_etc_qi as *const c_void,
    xtra_etc_addref as *const c_void,
    xtra_etc_release as *const c_void,
    xtra_etc_register as *const c_void,
]);

static SCRIPT_XTRA_VTABLE: SyncVTable = SyncVTable([
    script_xtra_qi as *const c_void,
    script_xtra_addref as *const c_void,
    script_xtra_release as *const c_void,
    script_xtra_call_handler as *const c_void,
]);

// ============================================================================
// Interface object layout: [vtable_ptr, parent_ptr, refcount]  (12 bytes)
// ============================================================================

#[repr(C)]
struct InterfaceObj {
    vtable: *const *const c_void,
    parent: *mut c_void,
    refcount: i32,
}

// ============================================================================
// XtraEtc vtable methods
// ============================================================================

unsafe extern "system" fn xtra_etc_qi(
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

unsafe extern "system" fn xtra_etc_addref(this: *mut InterfaceObj) -> u32 {
    if this.is_null() { return 0; }
    (*this).refcount += 1;
    (*this).refcount as u32
}

unsafe extern "system" fn xtra_etc_release(this: *mut InterfaceObj) -> u32 {
    if this.is_null() { return 0; }
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

/// Register — calls IMoaRegister::SetInfo with XTRA_INFO
unsafe extern "system" fn xtra_etc_register(
    _this: *mut InterfaceObj,
    p_register: *mut c_void,
) -> i32 {
    debug_log("KEYPOLL XtraEtc::Register called");
    if p_register.is_null() {
        return 0x80004003_u32 as i32;
    }
    // IMoaRegister vtable: [0]QI [1]AddRef [2]Release [3..5]other [6]SetInfo
    // SetInfo is at offset 0x18 = vtable[6] (confirmed via Ghidra decompilation)
    let vtable = *(p_register as *const *const *const c_void);
    let set_info_fn: unsafe extern "system" fn(
        *mut c_void, i32, *const u8, i32, *const u8,
    ) -> i32 = std::mem::transmute(*vtable.add(6));
    let result = set_info_fn(
        p_register,
        9,
        XTRA_INFO.as_ptr(),
        0,
        MSG_TABLE.as_ptr(),
    );
    debug_log(&format!("KEYPOLL Register SetInfo returned 0x{:08X}", result as u32));
    result
}

// ============================================================================
// ScriptXtra vtable methods
// ============================================================================

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

unsafe extern "system" fn script_xtra_addref(this: *mut InterfaceObj) -> u32 {
    if this.is_null() { return 0; }
    (*this).refcount += 1;
    (*this).refcount as u32
}

unsafe extern "system" fn script_xtra_release(this: *mut InterfaceObj) -> u32 {
    if this.is_null() { return 0; }
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

/// Helper: Get IMoaMmValue* from ScriptXtra instance (stored at inst+0x18)
unsafe fn get_imm_value(this: *mut InterfaceObj) -> *mut c_void {
    let parent = (*this).parent;
    if parent.is_null() { return ptr::null_mut(); }
    *((parent as *const u8).add(0x18) as *const *mut c_void)
}

/// Helper: Get IMoaMmList* from ScriptXtra instance (stored at inst+0x1C)
unsafe fn get_imm_list(this: *mut InterfaceObj) -> *mut c_void {
    let parent = (*this).parent;
    if parent.is_null() { return ptr::null_mut(); }
    *((parent as *const u8).add(0x1C) as *const *mut c_void)
}

/// Helper: Call IMoaMmValue::ValueToInteger (vtable[13], offset 0x34)
/// Converts a MoaMmValue to an integer.
unsafe fn moa_value_to_integer(imm_value: *mut c_void, value_ptr: *const c_void) -> i32 {
    let vtable = *(imm_value as *const *const *const c_void);
    let func: unsafe extern "system" fn(*mut c_void, *const c_void) -> i32 =
        std::mem::transmute(*vtable.add(13)); // offset 0x34
    func(imm_value, value_ptr)
}

/// Helper: Call IMoaMmValue::IntegerToValue (vtable[22], offset 0x58)
/// Writes an integer as a MoaMmValue into the target location.
unsafe fn moa_integer_to_value(imm_value: *mut c_void, int_val: i32, out_value: *mut c_void) -> i32 {
    let vtable = *(imm_value as *const *const *const c_void);
    let func: unsafe extern "system" fn(*mut c_void, i32, *mut c_void) -> i32 =
        std::mem::transmute(*vtable.add(22)); // offset 0x58
    func(imm_value, int_val, out_value)
}

/// Helper: Call IMoaMmValue::ValueRelease (vtable[12], offset 0x30)
unsafe fn moa_value_release(imm_value: *mut c_void, value_ptr: *mut c_void) {
    let vtable = *(imm_value as *const *const *const c_void);
    let func: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32 =
        std::mem::transmute(*vtable.add(12)); // offset 0x30
    func(imm_value, value_ptr);
}

/// Helper: Call IMoaMmList::NewListValue (vtable[3], offset 0x0C)
/// Creates a new empty list and writes it to out_value.
unsafe fn moa_new_list(imm_list: *mut c_void, out_value: *mut c_void) -> i32 {
    let vtable = *(imm_list as *const *const *const c_void);
    let func: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32 =
        std::mem::transmute(*vtable.add(3)); // offset 0x0C
    func(imm_list, out_value)
}

/// Helper: Call IMoaMmList::AppendValueToList (vtable[4], offset 0x10)
unsafe fn moa_list_append(imm_list: *mut c_void, list_value: *mut c_void, item_value: *mut c_void) -> i32 {
    let vtable = *(imm_list as *const *const *const c_void);
    let func: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void) -> i32 =
        std::mem::transmute(*vtable.add(4)); // offset 0x10
    func(imm_list, list_value, item_value)
}

/// CallHandler – dispatches Lingo messages to keyboard handlers
///
/// Handler IDs (1-based, from XTRA_INFO order):
///   1 = bgOneKey
///   2 = bgAllKeys
///   3 = hcKeysOff
///   4 = hcKeysOn
///
/// call_info layout:
///   +0x08: handler_id (i32)
///   +0x0C: return value slot (MoaMmValue, 8 bytes)
///   +0x18: pointer to argument array (MoaMmValue pairs)
unsafe extern "system" fn script_xtra_call_handler(
    this: *mut InterfaceObj,
    call_info: *mut c_void,
) -> i32 {
    if this.is_null() || call_info.is_null() {
        return 0x80004005_u32 as i32;
    }

    let ci = call_info as *mut u8;

    // Get handler_id from call_info at offset +8
    let msg_id = *(ci.add(8) as *const i32);

    debug_log(&format!("KEYPOLL CallHandler: msg_id={}", msg_id));

    match msg_id {
        1 => {
            // bgOneKey(keyCode) — check if a specific key is down
            // 1. Get IMoaMmValue* from instance data
            let imm_value = get_imm_value(this);
            if imm_value.is_null() {
                debug_log("bgOneKey: no IMoaMmValue interface!");
                return 0;
            }

            // 2. Read argument: MoaMmValue at *(call_info+0x18)
            //    The args array pointer is at call_info+0x18
            //    First arg MoaMmValue (8 bytes) is at args_ptr+0 (or +8 in some layouts)
            let args_ptr = *(ci.add(0x18) as *const *const u8);
            if args_ptr.is_null() {
                debug_log("bgOneKey: null args pointer");
                return 0;
            }

            // 3. Convert MoaMmValue → integer via ValueToInteger (vtable[13])
            let vkey = moa_value_to_integer(imm_value, args_ptr as *const c_void);
            debug_log(&format!("bgOneKey: vkey={}", vkey));

            // 4. Call GetAsyncKeyState
            let is_down = keyboard::is_key_down(vkey);
            let result_int = if is_down { 1 } else { 0 };

            // 5. Write result via IntegerToValue → call_info+0x0C
            let ret_slot = ci.add(0x0C) as *mut c_void;
            moa_integer_to_value(imm_value, result_int, ret_slot);

            debug_log(&format!("bgOneKey: result={}", result_int));
            0
        }
        2 => {
            // bgAllKeys() — return linear list of all pressed key codes
            let imm_value = get_imm_value(this);
            let imm_list = get_imm_list(this);
            if imm_value.is_null() || imm_list.is_null() {
                debug_log("bgAllKeys: missing IMoaMmValue or IMoaMmList!");
                return 0;
            }

            // 1. Create new list in the return slot (call_info+0x0C)
            let ret_slot = ci.add(0x0C) as *mut c_void;
            let hr = moa_new_list(imm_list, ret_slot);
            if hr != 0 {
                debug_log(&format!("bgAllKeys: NewListValue failed: 0x{:08X}", hr as u32));
                return 0;
            }

            // 2. Get all pressed keys using keyboard module
            let pressed_keys = keyboard::get_all_keys_down();
            let mut temp_value: [u8; 8] = [0; 8];
            for vkey in pressed_keys {
                // Convert int → MoaMmValue
                moa_integer_to_value(
                    imm_value,
                    vkey,
                    temp_value.as_mut_ptr() as *mut c_void,
                );
                // Append to list
                moa_list_append(
                    imm_list,
                    ret_slot,
                    temp_value.as_mut_ptr() as *mut c_void,
                );
                // Release temp value
                moa_value_release(imm_value, temp_value.as_mut_ptr() as *mut c_void);
                temp_value = [0; 8]; // re-zero for next iteration
            }

            debug_log("bgAllKeys: list built");
            0
        }
        3 => {
            // hcKeysOff — block keyboard messages
            debug_log(&format!("hcKeysOff: hook already active = {}", keyboard::is_hook_active()));
            keyboard::keys_off();
            0
        }
        4 => {
            // hcKeysOn — unblock keyboard messages
            debug_log(&format!("hcKeysOn: hook was active = {}", keyboard::is_hook_active()));
            keyboard::keys_on();
            0
        }
        _ => {
            debug_log(&format!("KEYPOLL: unknown handler {}", msg_id));
            0
        }
    }
}

// ============================================================================
// Class Create / Destroy callbacks
// ============================================================================

unsafe extern "C" fn class1_create(this: *mut c_void) -> i32 {
    debug_log("KEYPOLL class1_create (XtraEtc)");
    if !this.is_null() {
        ptr::write_bytes(this as *mut u8, 0, CLASS1_INST_SIZE as usize);
    }
    0
}

unsafe extern "C" fn class1_destroy(this: *mut c_void) -> i32 {
    debug_log("KEYPOLL class1_destroy");
    let _ = this;
    0
}

/// Class2 (ScriptXtra) create callback
///
/// Instance layout (0x2C = 44 bytes):
///   +0x00..+0x17: Director-managed fields (DO NOT OVERWRITE!)
///     +0x08: pCallback* (IMoaUnknown for QueryInterface)
///   +0x18: IMoaMmValue* (queried via QI)
///   +0x1C: IMoaMmList* (queried via QI)
///   +0x20: hookEnabled (1=ready, 0=hook active)
///   +0x24: HHOOK (keyboard hook handle)
///   +0x28: HOOKPROC (hook procedure address)
unsafe extern "C" fn class2_create(this: *mut c_void) -> i32 {
    debug_log("KEYPOLL class2_create (ScriptXtra)");
    if this.is_null() {
        return 0;
    }

    let p = this as *mut u8;

    // IMPORTANT: Do NOT zero the entire instance!
    // Director has pre-filled +0x00..+0x0C with internal fields including
    // the pCallback pointer at +0x08 that we need for QueryInterface.
    // Only zero what we own: +0x18..+0x2B
    ptr::write_bytes(p.add(0x18), 0, 0x2C - 0x18);

    // Get pCallback from instance+0x08 (set by Director)
    let p_callback = *(p.add(0x08) as *const *mut c_void);
    if p_callback.is_null() {
        debug_log("KEYPOLL class2_create: pCallback is null!");
        return 0;
    }

    // QueryInterface for IMoaMmValue → store at +0x18
    let cb_vtable = *(p_callback as *const *const *const c_void);
    let qi_fn: unsafe extern "system" fn(
        *mut c_void, *const u8, *mut *mut c_void,
    ) -> i32 = std::mem::transmute(*cb_vtable); // vtable[0] = QueryInterface

    let mut imm_value: *mut c_void = ptr::null_mut();
    let hr = qi_fn(p_callback, IID_MOA_MM_VALUE.as_ptr(), &mut imm_value);
    if hr == 0 && !imm_value.is_null() {
        *(p.add(0x18) as *mut *mut c_void) = imm_value;
        debug_log("KEYPOLL: got IMoaMmValue interface");
    } else {
        debug_log(&format!("KEYPOLL: failed to get IMoaMmValue: 0x{:08X}", hr as u32));
    }

    // QueryInterface for IMoaMmList → store at +0x1C
    let mut imm_list: *mut c_void = ptr::null_mut();
    let hr = qi_fn(p_callback, IID_MOA_MM_LIST.as_ptr(), &mut imm_list);
    if hr == 0 && !imm_list.is_null() {
        *(p.add(0x1C) as *mut *mut c_void) = imm_list;
        debug_log("KEYPOLL: got IMoaMmList interface");
    } else {
        debug_log(&format!("KEYPOLL: failed to get IMoaMmList: 0x{:08X}", hr as u32));
    }

    // Initialize hook state
    *(p.add(0x20) as *mut i32) = 1;  // hookEnabled = ready
    *(p.add(0x24) as *mut i32) = 0;  // HHOOK = null
    *(p.add(0x28) as *mut i32) = 0;  // HOOKPROC = null

    0
}

/// Class2 (ScriptXtra) destroy callback
/// Releases IMoaMmValue and IMoaMmList interfaces acquired in Create.
unsafe extern "C" fn class2_destroy(this: *mut c_void) -> i32 {
    debug_log("KEYPOLL class2_destroy");
    if this.is_null() {
        return 0;
    }

    let p = this as *mut u8;

    // Release IMoaMmValue at +0x18
    let imm_value = *(p.add(0x18) as *const *mut c_void);
    if !imm_value.is_null() {
        let vtable = *(imm_value as *const *const *const c_void);
        let release_fn: unsafe extern "system" fn(*mut c_void) -> u32 =
            std::mem::transmute(*vtable.add(2)); // vtable[2] = Release
        release_fn(imm_value);
    }

    // Release IMoaMmList at +0x1C
    let imm_list = *(p.add(0x1C) as *const *mut c_void);
    if !imm_list.is_null() {
        let vtable = *(imm_list as *const *const *const c_void);
        let release_fn: unsafe extern "system" fn(*mut c_void) -> u32 =
            std::mem::transmute(*vtable.add(2));
        release_fn(imm_list);
    }

    // Remove keyboard hook if still active
    keyboard::keys_on();

    0
}

// ============================================================================
// Interface factory functions
// ============================================================================

unsafe extern "C" fn create_xtra_etc_interface_factory(
    parent: *mut c_void,
    ppv: *mut *mut c_void,
) -> i32 {
    debug_log("KEYPOLL create_xtra_etc_interface_factory");
    if ppv.is_null() {
        return 0x80004003_u32 as i32;
    }

    let obj = Box::new(InterfaceObj {
        vtable: XTRA_ETC_VTABLE.as_ptr(),
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

unsafe extern "C" fn create_script_xtra_interface_factory(
    parent: *mut c_void,
    ppv: *mut *mut c_void,
) -> i32 {
    debug_log("KEYPOLL create_script_xtra_interface_factory");
    if ppv.is_null() {
        return 0x80004003_u32 as i32;
    }

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
// Central registration function
// ============================================================================

type ClassCallback = unsafe extern "C" fn(
    guid: *const u8, flags: u32, inst_size: u32,
    create_fn: *const c_void, destroy_fn: *const c_void, user_data: *mut c_void,
) -> i32;

type InterfaceCallback = unsafe extern "C" fn(
    class_guid: *const u8, class_flags: u32,
    iid: *const u8, iface_flags: u32,
    factory_fn: *const c_void, user_data: *mut c_void,
) -> i32;

unsafe fn register_classes(
    filter: *const u8,
    class_cb: Option<ClassCallback>,
    iface_cb: Option<InterfaceCallback>,
    user_data: *mut c_void,
) -> i32 {
    // --- Class 1: XtraEtc ---
    if filter.is_null() || guid_eq(filter, &CLASS1_GUID) {
        if let Some(cb) = class_cb {
            let r = cb(CLASS1_GUID.as_ptr(), VERSION_FLAGS, CLASS1_INST_SIZE,
                      class1_create as *const c_void, class1_destroy as *const c_void, user_data);
            if r != 0 { return r; }
        }
        if let Some(cb) = iface_cb {
            let r = cb(CLASS1_GUID.as_ptr(), VERSION_FLAGS, IID_XTRA_ETC.as_ptr(),
                      VERSION_FLAGS, create_xtra_etc_interface_factory as *const c_void, user_data);
            if r != 0 { return r; }
        }
    }

    // --- Class 2: ScriptXtra ---
    if filter.is_null() || guid_eq(filter, &CLASS2_GUID) {
        if let Some(cb) = class_cb {
            let r = cb(CLASS2_GUID.as_ptr(), VERSION_FLAGS, CLASS2_INST_SIZE,
                      class2_create as *const c_void, class2_destroy as *const c_void, user_data);
            if r != 0 { return r; }
        }
        if let Some(cb) = iface_cb {
            let r = cb(CLASS2_GUID.as_ptr(), VERSION_FLAGS, IID_SCRIPT.as_ptr(),
                      VERSION_FLAGS, create_script_xtra_interface_factory as *const c_void, user_data);
            if r != 0 { return r; }
        }
    }

    0
}

// ============================================================================
// DLL Exports — extern "C" to avoid @N decoration
// ============================================================================

/// Export 1: MyKeyProc — keyboard hook stub, returns 1
/// The real hook is installed/removed via hcKeysOff/hcKeysOn handlers.
#[no_mangle]
pub unsafe extern "C" fn MyKeyProc() -> i32 {
    1
}

/// Export 2: DllGetClassObject — stub, always returns CLASS_E_CLASSNOTAVAILABLE
#[no_mangle]
pub unsafe extern "C" fn DllGetClassObject(
    _rclsid: *const u8,
    _riid: *const u8,
    _ppv: *mut *mut c_void,
) -> i32 {
    debug_log("KEYPOLL DllGetClassObject called (stub)");
    0x80040005_u32 as i32
}

/// Export 3: DllCanUnloadNow
#[no_mangle]
pub unsafe extern "C" fn DllCanUnloadNow() -> i32 {
    let rc = GLOBAL_REFCOUNT.load(Ordering::SeqCst);
    if rc == 0 { 0 } else { 0x80040003_u32 as i32 }
}

/// Export 4: DllGetClassForm
#[no_mangle]
pub unsafe extern "C" fn DllGetClassForm(
    filter: *const u8,
    out_inst_size: *mut u32,
    out_create_fn: *mut *const c_void,
    out_destroy_fn: *mut *const c_void,
) -> i32 {
    debug_log("KEYPOLL DllGetClassForm called");

    #[repr(C)]
    struct FormUserData {
        out_inst_size: *mut u32,
        out_create_fn: *mut *const c_void,
        out_destroy_fn: *mut *const c_void,
        found: bool,
    }

    unsafe extern "C" fn form_class_cb(
        _guid: *const u8, _flags: u32, inst_size: u32,
        create_fn: *const c_void, destroy_fn: *const c_void, user_data: *mut c_void,
    ) -> i32 {
        let data = &mut *(user_data as *mut FormUserData);
        if !data.out_inst_size.is_null() { *data.out_inst_size = inst_size; }
        if !data.out_create_fn.is_null() { *data.out_create_fn = create_fn; }
        if !data.out_destroy_fn.is_null() { *data.out_destroy_fn = destroy_fn; }
        data.found = true;
        0
    }

    let mut data = FormUserData { out_inst_size, out_create_fn, out_destroy_fn, found: false };
    register_classes(filter, Some(form_class_cb), None, &mut data as *mut FormUserData as *mut c_void);
    if data.found { 0 } else { 0x80040005_u32 as i32 }
}

/// Export 5: DllGetInterface
#[no_mangle]
pub unsafe extern "C" fn DllGetInterface(
    parent: *mut c_void,
    class_guid: *const u8,
    iid: *const u8,
    ppv: *mut *mut c_void,
) -> i32 {
    debug_log("KEYPOLL DllGetInterface called");
    if ppv.is_null() {
        return 0x80004003_u32 as i32;
    }

    #[repr(C)]
    struct IfaceUserData {
        iid: *const u8,
        factory: Option<unsafe extern "C" fn(*mut c_void, *mut *mut c_void) -> i32>,
    }

    unsafe extern "C" fn iface_find_cb(
        _class_guid: *const u8, _class_flags: u32,
        cb_iid: *const u8, _iface_flags: u32,
        factory_fn: *const c_void, user_data: *mut c_void,
    ) -> i32 {
        let data = &mut *(user_data as *mut IfaceUserData);
        if guid_eq(data.iid, &*(cb_iid as *const [u8; 16])) {
            data.factory = Some(std::mem::transmute(factory_fn));
        }
        0
    }

    let mut data = IfaceUserData { iid, factory: None };
    register_classes(class_guid, None, Some(iface_find_cb), &mut data as *mut IfaceUserData as *mut c_void);

    match data.factory {
        Some(factory) => factory(parent, ppv),
        None => {
            *ppv = ptr::null_mut();
            0x80040004_u32 as i32
        }
    }
}

/// Export 6: DllGetClassInfo
#[no_mangle]
pub unsafe extern "C" fn DllGetClassInfo(
    calloc: *mut c_void,
    pp_class_info: *mut *mut c_void,
) -> i32 {
    debug_log("KEYPOLL DllGetClassInfo called");
    if calloc.is_null() || pp_class_info.is_null() {
        return 0x80004003_u32 as i32;
    }

    let entry_size: u32 = 0x28;
    let total_size = entry_size * 3; // 2 entries + terminator

    let vtable = *(calloc as *const *const *const c_void);
    let nr_alloc: unsafe extern "system" fn(*mut c_void, u32) -> *mut c_void =
        std::mem::transmute(*vtable.add(3));
    let buf = nr_alloc(calloc, total_size);
    if buf.is_null() {
        return 0x80040002_u32 as i32;
    }

    ptr::write_bytes(buf as *mut u8, 0, total_size as usize);

    let entry0 = buf as *mut u8;
    ptr::copy_nonoverlapping(CLASS1_GUID.as_ptr(), entry0, 16);
    ptr::copy_nonoverlapping(IID_XTRA_ETC.as_ptr(), entry0.add(0x10), 16);

    let entry1 = entry0.add(entry_size as usize);
    ptr::copy_nonoverlapping(CLASS2_GUID.as_ptr(), entry1, 16);
    ptr::copy_nonoverlapping(IID_SCRIPT.as_ptr(), entry1.add(0x10), 16);

    *pp_class_info = buf;
    0
}
