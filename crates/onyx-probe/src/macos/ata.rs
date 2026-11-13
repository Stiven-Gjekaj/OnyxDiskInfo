//! The macOS shim for ATA SMART data. Experimental: no SATA drive has
//! answered it.
//!
//! macOS gives no ATA pass-through. It gives a plugin instead,
//! `ATASMARTLib.plugin`, on the node of the drive that carries the property
//! `SMART Capable`. An internal SATA drive of an Intel Mac has that node. A
//! SATA drive in a USB case has it only when a SAT driver is installed, such
//! as the one of OS X SAT SMART Driver. The table of calls below is the one
//! in `IOKit/storage/ata/ATASMARTLib.h` of the macOS SDK.
//!
//! The plugin is a COM interface, as the NVMe one in `smart.rs` is.

#![allow(unsafe_code)]

use std::ffi::c_void;

use core_foundation_sys::base::CFRelease;

use onyx_core::{Error, Result};

use super::smart::{
    CF_PLUGIN_INTERFACE, IOCreatePlugInInterfaceForService, IODestroyPlugInInterface, PlugIn,
    QueryInterface, Release, capable_node, uuid, uuid_bytes,
};
use crate::health::RawHealth;

/// `kIOATASMARTUserClientTypeID`: 24514B7A-2804-11D6-8A02-003065704866.
const ATA_SMART_USER_CLIENT: [u8; 16] = [
    0x24, 0x51, 0x4b, 0x7a, 0x28, 0x04, 0x11, 0xd6, 0x8a, 0x02, 0x00, 0x30, 0x65, 0x70, 0x48, 0x66,
];

/// `kIOATASMARTInterfaceID`: 08ABE21C-20D4-11D6-8DF6-0003935A76B2.
const ATA_SMART_INTERFACE: [u8; 16] = [
    0x08, 0xab, 0xe2, 0x1c, 0x20, 0xd4, 0x11, 0xd6, 0x8d, 0xf6, 0x00, 0x03, 0x93, 0x5a, 0x76, 0xb2,
];

/// `kIOPropertySMARTCapableKey`: the property that marks the node that
/// carries the plugin.
const CAPABLE: &str = "SMART Capable";

/// The size of IDENTIFY DEVICE, of SMART READ DATA, and of SMART READ
/// THRESHOLDS, in bytes.
const BLOCK: usize = 512;

type Call = unsafe extern "C" fn(this: *mut c_void, data: *mut u8) -> i32;
type Toggle = unsafe extern "C" fn(this: *mut c_void, on: u8) -> i32;

/// `IOATASMARTInterface`, up to `GetATAIdentifyData`. Each entry keeps its
/// place in the table, so the calls that Onyx does not make are here too.
#[repr(C)]
struct AtaSmart {
    _reserved: *mut c_void,
    _query_interface: QueryInterface,
    _add_ref: Release,
    release: Release,
    _version: u16,
    _revision: u16,
    _enable_disable_operations: Toggle,
    _enable_disable_autosave: Toggle,
    _return_status: Call,
    _execute_off_line_immediate: Toggle,
    smart_read_data: Call,
    _validate_read_data: Call,
    smart_read_data_thresholds: Call,
    _read_log_directory: Call,
    _read_log_at_address: unsafe extern "C" fn(*mut c_void, u32, *mut c_void, u32) -> i32,
    _write_log_at_address: unsafe extern "C" fn(*mut c_void, u32, *const c_void, u32) -> i32,
    get_ata_identify_data:
        unsafe extern "C" fn(this: *mut c_void, data: *mut c_void, size: u32, out: *mut u32) -> i32,
}

/// Read the identify data, the SMART data, and the thresholds of a SATA
/// drive.
///
/// `Ok(None)` means that no node above the drive carries the plugin, so
/// macOS gives no ATA SMART data for the drive.
pub fn ata_health(bsd_name: &str) -> Result<Option<RawHealth>> {
    let Some(node) = capable_node(bsd_name, CAPABLE)? else {
        return Ok(None);
    };

    let plugin_type = uuid(ATA_SMART_USER_CLIENT);
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

    let mut smart: *mut *mut AtaSmart = std::ptr::null_mut();
    // SAFETY: the plugin is live, and QueryInterface is the second entry of
    // every COM table.
    let status = unsafe {
        ((**plugin).query_interface)(
            plugin.cast(),
            uuid_bytes(ATA_SMART_INTERFACE),
            (&mut smart as *mut *mut *mut AtaSmart).cast(),
        )
    };
    let result = if status != 0 || smart.is_null() {
        Err(Error::host(
            "QueryInterface",
            format!("the plugin refused the ATA SMART interface: {status:#x}"),
        ))
    } else {
        let mut identify = vec![0u8; BLOCK];
        let mut data = vec![0u8; BLOCK];
        let mut thresholds = vec![0u8; BLOCK];
        let mut identify_bytes = 0u32;
        // SAFETY: the interface is live. Each buffer holds 512 bytes, which
        // is the size of each structure that the calls write.
        let (id_status, data_status, threshold_status) = unsafe {
            let table = &**smart;
            let id_status = (table.get_ata_identify_data)(
                smart.cast(),
                identify.as_mut_ptr().cast(),
                BLOCK as u32,
                &mut identify_bytes,
            );
            let data_status = (table.smart_read_data)(smart.cast(), data.as_mut_ptr());
            let threshold_status =
                (table.smart_read_data_thresholds)(smart.cast(), thresholds.as_mut_ptr());
            (table.release)(smart.cast());
            (id_status, data_status, threshold_status)
        };
        if id_status != 0 {
            Err(Error::host(
                "GetATAIdentifyData",
                format!("the call returned {id_status:#x}"),
            ))
        } else if identify_bytes as usize != BLOCK {
            Err(Error::host(
                "GetATAIdentifyData",
                format!("the call gave {identify_bytes} bytes and not {BLOCK}"),
            ))
        } else if data_status != 0 {
            Err(Error::host(
                "SMARTReadData",
                format!("the call returned {data_status:#x}"),
            ))
        } else {
            Ok(Some(RawHealth::Ata {
                identify,
                smart: data,
                thresholds: (threshold_status == 0).then_some(thresholds),
            }))
        }
    };
    // SAFETY: the plugin was created above and is destroyed once, here.
    unsafe { IODestroyPlugInInterface(plugin) };
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_holds_each_call_up_to_the_identify_call() {
        // Four pointers and two u16 values, padded to a pointer, then eleven
        // calls. A missing entry moves every call after it.
        let pointer = std::mem::size_of::<*mut c_void>();
        assert_eq!(std::mem::size_of::<AtaSmart>(), (4 + 1 + 11) * pointer);
    }
}
