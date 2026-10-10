//! The macOS shim for NVMe health data.
//!
//! macOS gives no pass-through for NVMe commands. It gives a plugin instead,
//! `NVMeSMARTLib.plugin`, on the node of the drive that carries the property
//! `NVMe SMART Capable`. The plugin has a call for the health log and a call
//! for the identify data. smartmontools uses the same plugin, and the layout
//! of its table of calls below is the one that smartmontools uses.
//!
//! The plugin is a COM interface: a pointer to a pointer to a table of
//! function pointers. Each call is unsafe, and each one says why it is sound.

#![allow(unsafe_code)]

use std::ffi::{CString, c_void};

use core_foundation::base::{CFType, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::string::CFString;
use core_foundation_sys::base::{CFRelease, kCFAllocatorDefault};
use core_foundation_sys::uuid::{CFUUIDBytes, CFUUIDCreateFromUUIDBytes, CFUUIDRef};
use io_kit_sys::keys::kIOServicePlane;
use io_kit_sys::types::io_object_t;
use io_kit_sys::{
    IOBSDNameMatching, IOObjectRelease, IORegistryEntryCreateCFProperty,
    IORegistryEntryGetParentEntry, IOServiceGetMatchingService, kIOMasterPortDefault,
};

use onyx_core::{Error, Result};

use crate::health::RawHealth;

/// `kIONVMeSMARTUserClientTypeID`: AA0FA6F9-C2D6-457F-B10B-59A13253292F.
const NVME_SMART_USER_CLIENT: [u8; 16] = [
    0xaa, 0x0f, 0xa6, 0xf9, 0xc2, 0xd6, 0x45, 0x7f, 0xb1, 0x0b, 0x59, 0xa1, 0x32, 0x53, 0x29, 0x2f,
];

/// `kIONVMeSMARTInterfaceID`: CCD1DB19-FD9A-4DAF-BF95-12454B230AB6.
const NVME_SMART_INTERFACE: [u8; 16] = [
    0xcc, 0xd1, 0xdb, 0x19, 0xfd, 0x9a, 0x4d, 0xaf, 0xbf, 0x95, 0x12, 0x45, 0x4b, 0x23, 0x0a, 0xb6,
];

/// `kIOCFPlugInInterfaceID`: C244E858-109C-11D4-91D4-0050E4C6426F.
const CF_PLUGIN_INTERFACE: [u8; 16] = [
    0xc2, 0x44, 0xe8, 0x58, 0x10, 0x9c, 0x11, 0xd4, 0x91, 0xd4, 0x00, 0x50, 0xe4, 0xc6, 0x42, 0x6f,
];

/// The property that marks the node that carries the plugin.
const CAPABLE: &str = "NVMe SMART Capable";

/// The size of the Identify Controller data, in bytes.
const IDENTIFY_BYTES: usize = 4096;

/// The size of the health log, in bytes.
const LOG_BYTES: usize = 512;

type QueryInterface =
    unsafe extern "C" fn(this: *mut c_void, iid: CFUUIDBytes, out: *mut *mut c_void) -> i32;
type Release = unsafe extern "C" fn(this: *mut c_void) -> u32;

/// The start of every COM interface: `IUNKNOWN_C_GUTS`.
#[repr(C)]
struct PlugIn {
    _reserved: *mut c_void,
    query_interface: QueryInterface,
    _add_ref: Release,
    release: Release,
}

/// `IONVMeSMARTInterface`, up to the two calls that Onyx makes. More calls
/// follow in the real table, and Onyx reads none of them.
#[repr(C)]
struct NvmeSmart {
    _reserved: *mut c_void,
    _query_interface: QueryInterface,
    _add_ref: Release,
    release: Release,
    _version: u16,
    _revision: u16,
    smart_read_data: unsafe extern "C" fn(this: *mut c_void, data: *mut u8) -> i32,
    get_identify_data: unsafe extern "C" fn(this: *mut c_void, data: *mut u8, ns: u32) -> i32,
}

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOCreatePlugInInterfaceForService(
        service: io_object_t,
        plugin_type: CFUUIDRef,
        interface_type: CFUUIDRef,
        the_interface: *mut *mut *mut PlugIn,
        the_score: *mut i32,
    ) -> i32;
    fn IODestroyPlugInInterface(interface: *mut *mut PlugIn) -> i32;
}

/// An IOKit object that this code owns, released when it goes.
struct Object(io_object_t);

impl Drop for Object {
    fn drop(&mut self) {
        if self.0 != 0 {
            // SAFETY: the object is owned here and released once.
            unsafe { IOObjectRelease(self.0) };
        }
    }
}

fn uuid(bytes: [u8; 16]) -> CFUUIDRef {
    let [
        byte0,
        byte1,
        byte2,
        byte3,
        byte4,
        byte5,
        byte6,
        byte7,
        byte8,
        byte9,
        byte10,
        byte11,
        byte12,
        byte13,
        byte14,
        byte15,
    ] = bytes;
    let raw = CFUUIDBytes {
        byte0,
        byte1,
        byte2,
        byte3,
        byte4,
        byte5,
        byte6,
        byte7,
        byte8,
        byte9,
        byte10,
        byte11,
        byte12,
        byte13,
        byte14,
        byte15,
    };
    // SAFETY: the allocator is the default one, and the bytes are a value.
    unsafe { CFUUIDCreateFromUUIDBytes(kCFAllocatorDefault, raw) }
}

fn uuid_bytes(bytes: [u8; 16]) -> CFUUIDBytes {
    let id = uuid(bytes);
    // SAFETY: the UUID was created above and is released once, here.
    let out = unsafe { core_foundation_sys::uuid::CFUUIDGetUUIDBytes(id) };
    // SAFETY: as above.
    unsafe { CFRelease(id as *const c_void) };
    out
}

/// Whether a node carries the property that marks the plugin.
fn is_capable(entry: io_object_t) -> bool {
    let key = CFString::new(CAPABLE);
    // SAFETY: the entry is live and the key is a valid string. The call
    // creates the value, so the wrapper takes it under the create rule.
    let value = unsafe {
        IORegistryEntryCreateCFProperty(entry, key.as_concrete_TypeRef(), kCFAllocatorDefault, 0)
    };
    if value.is_null() {
        return false;
    }
    // SAFETY: the value was created by the call above.
    let value = unsafe { CFType::wrap_under_create_rule(value) };
    value
        .downcast::<CFBoolean>()
        .is_some_and(|b| b == CFBoolean::true_value())
}

/// Find the node above a whole disk that carries the plugin.
fn capable_node(bsd_name: &str) -> Result<Option<Object>> {
    let name = CString::new(bsd_name)
        .map_err(|_| Error::host("IOBSDNameMatching", "a zero byte in the name"))?;
    // SAFETY: the name is a valid C string. The matching dictionary goes to
    // IOServiceGetMatchingService, which releases it.
    let media = unsafe {
        let matching = IOBSDNameMatching(kIOMasterPortDefault, 0, name.as_ptr());
        IOServiceGetMatchingService(kIOMasterPortDefault, matching)
    };
    if media == 0 {
        return Err(Error::NoSuchDrive {
            wanted: bsd_name.to_string(),
        });
    }
    let mut here = Object(media);
    // The registry is a few levels deep. The count guards against a loop.
    for _ in 0..32 {
        if is_capable(here.0) {
            return Ok(Some(here));
        }
        let mut parent: io_object_t = 0;
        // SAFETY: `here` is live, and the plane is a constant C string.
        let status = unsafe { IORegistryEntryGetParentEntry(here.0, kIOServicePlane, &mut parent) };
        if status != 0 || parent == 0 {
            return Ok(None);
        }
        here = Object(parent);
    }
    Ok(None)
}

/// Read the identify data and the health log of an NVMe drive.
///
/// `Ok(None)` means that no node above the drive carries the plugin, so the
/// drive is not an NVMe drive that macOS reads SMART from.
pub fn nvme_health(bsd_name: &str) -> Result<Option<RawHealth>> {
    let Some(node) = capable_node(bsd_name)? else {
        return Ok(None);
    };

    let plugin_type = uuid(NVME_SMART_USER_CLIENT);
    let interface_type = uuid(CF_PLUGIN_INTERFACE);
    let mut plugin: *mut *mut PlugIn = std::ptr::null_mut();
    let mut score = 0i32;
    // SAFETY: the node is live, both UUIDs are valid, and the out pointers
    // point at locals.
    let status = unsafe {
        IOCreatePlugInInterfaceForService(
            node.0,
            plugin_type,
            interface_type,
            &mut plugin,
            &mut score,
        )
    };
    // SAFETY: both UUIDs were created above and are released once, here.
    unsafe {
        CFRelease(plugin_type as *const c_void);
        CFRelease(interface_type as *const c_void);
    }
    if status != 0 || plugin.is_null() {
        return Err(Error::host(
            "IOCreatePlugInInterfaceForService",
            format!("the call returned {status:#x}"),
        ));
    }

    let mut smart: *mut *mut NvmeSmart = std::ptr::null_mut();
    // SAFETY: the plugin is live, and QueryInterface is the second entry of
    // every COM table.
    let status = unsafe {
        ((**plugin).query_interface)(
            plugin.cast(),
            uuid_bytes(NVME_SMART_INTERFACE),
            (&mut smart as *mut *mut *mut NvmeSmart).cast(),
        )
    };
    let result = if status != 0 || smart.is_null() {
        Err(Error::host(
            "QueryInterface",
            format!("the plugin refused the NVMe SMART interface: {status:#x}"),
        ))
    } else {
        let mut identify = vec![0u8; IDENTIFY_BYTES];
        let mut log = vec![0u8; LOG_BYTES];
        // SAFETY: the interface is live. Each buffer has the size that its
        // call writes: 4096 bytes of identify data and 512 bytes of log.
        let (id_status, log_status) = unsafe {
            let table = &**smart;
            let id_status = (table.get_identify_data)(smart.cast(), identify.as_mut_ptr(), 0);
            let log_status = (table.smart_read_data)(smart.cast(), log.as_mut_ptr());
            (table.release)(smart.cast());
            (id_status, log_status)
        };
        if id_status != 0 {
            Err(Error::host(
                "GetIdentifyData",
                format!("the call returned {id_status:#x}"),
            ))
        } else if log_status != 0 {
            Err(Error::host(
                "SMARTReadData",
                format!("the call returned {log_status:#x}"),
            ))
        } else {
            Ok(Some(RawHealth::Nvme { identify, log }))
        }
    };
    // SAFETY: the plugin was created above and is destroyed once, here.
    unsafe { IODestroyPlugInInterface(plugin) };
    result
}
