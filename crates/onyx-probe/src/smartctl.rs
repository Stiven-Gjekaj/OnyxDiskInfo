//! The fallback to `smartctl`, for a drive that Onyx cannot read itself.
//!
//! smartmontools knows bridges and RAID controllers that Onyx does not know
//! yet. When `smartctl` is on the computer, Onyx runs it, reads its JSON, and
//! builds the same report that its own decoders build. Onyx links no code of
//! smartmontools. It runs the program and reads what it prints.
//!
//! A report from here has a medium confidence: the drive gave the values,
//! but Onyx cannot check how another program decoded them.

use std::process::Command;

use onyx_core::ata::{Attribute, Identity, Smart};
use onyx_core::nvme::{Controller, HealthLog};
use onyx_core::{DriveInfo, Report};
use serde_json::Value;

/// The host, as far as the name of a drive for `smartctl` depends on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    Linux,
    Macos,
    Windows,
}

/// The name that `smartctl` takes for a drive.
///
/// Linux takes the device node. macOS takes the BSD name. Windows takes
/// `/dev/pdN` for `\\.\PhysicalDriveN`.
pub fn device_name(host: Host, drive: &DriveInfo) -> String {
    match host {
        Host::Linux => drive.node.clone(),
        Host::Macos => drive.id.as_str().to_string(),
        Host::Windows => format!("/dev/pd{}", drive.id),
    }
}

fn text(json: &Value, key: &str) -> Option<String> {
    json.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

fn number(json: &Value, key: &str) -> u128 {
    json.get(key).and_then(Value::as_u64).map_or(0, u128::from)
}

fn small(json: &Value, key: &str) -> u8 {
    json.get(key)
        .and_then(Value::as_u64)
        .and_then(|v| u8::try_from(v).ok())
        .unwrap_or(0)
}

/// Build a report from the JSON of `smartctl --json -a`.
///
/// `None` means `smartctl` read no health data either: the JSON holds no
/// NVMe log and no ATA attribute table.
pub fn report(json: &Value) -> Option<Report> {
    if let Some(log) = json.get("nvme_smart_health_information_log") {
        return Some(nvme(json, log));
    }
    json.get("ata_smart_attributes")
        .and_then(|a| a.get("table"))
        .and_then(Value::as_array)
        .map(|table| ata(json, table))
}

fn nvme(json: &Value, log: &Value) -> Report {
    let mut controller = Controller::default();
    controller.model = text(json, "model_name");
    controller.serial = text(json, "serial_number");
    controller.firmware = text(json, "firmware_version");

    let mut health = HealthLog::default();
    health.critical_warning = small(log, "critical_warning");
    // smartctl gives degrees Celsius, and the log keeps kelvin.
    health.temperature_kelvin = log
        .get("temperature")
        .and_then(Value::as_i64)
        .and_then(|c| u16::try_from(c + 273).ok())
        .unwrap_or(0);
    health.available_spare = small(log, "available_spare");
    health.available_spare_threshold = small(log, "available_spare_threshold");
    health.percentage_used = small(log, "percentage_used");
    health.data_units_read = number(log, "data_units_read");
    health.data_units_written = number(log, "data_units_written");
    health.host_read_commands = number(log, "host_reads");
    health.host_write_commands = number(log, "host_writes");
    health.controller_busy_minutes = number(log, "controller_busy_time");
    health.power_cycles = number(log, "power_cycles");
    health.power_on_hours = number(log, "power_on_hours");
    health.unsafe_shutdowns = number(log, "unsafe_shutdowns");
    health.media_errors = number(log, "media_errors");
    health.error_log_entries = number(log, "num_err_log_entries");
    Report::Nvme {
        controller,
        log: health,
    }
}

fn ata(json: &Value, table: &[Value]) -> Report {
    let mut identity = Identity::default();
    identity.model = text(json, "model_name");
    identity.serial = text(json, "serial_number");
    identity.firmware = text(json, "firmware_version");
    identity.smart_supported = true;
    identity.smart_enabled = true;

    let mut smart = Smart::default();
    smart.attributes = table
        .iter()
        .filter_map(|a| {
            let id = u8::try_from(a.get("id")?.as_u64()?).ok()?;
            let flags = a
                .get("flags")
                .and_then(|f| f.get("value"))
                .and_then(Value::as_u64)
                .and_then(|v| u16::try_from(v).ok())
                .unwrap_or(0);
            let raw = a
                .get("raw")
                .and_then(|r| r.get("value"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            let threshold = a
                .get("thresh")
                .and_then(Value::as_u64)
                .and_then(|t| u8::try_from(t).ok());
            Some(Attribute::new(
                id,
                flags,
                small(a, "value"),
                small(a, "worst"),
                raw,
                threshold,
            ))
        })
        .collect();
    Report::Ata { identity, smart }
}

/// Run `smartctl --json -a` on one drive and read its answer.
///
/// `None` means that `smartctl` is not on the computer, that it printed no
/// JSON, or that it read no health data. None of these is an error of Onyx.
/// `smartctl` sets bits in its exit code for warnings as well as for
/// failures, so the JSON decides and not the code.
pub fn run(device: &str) -> Option<Report> {
    let output = Command::new("smartctl")
        .args(["--json", "-a", device])
        .output()
        .ok()?;
    let json: Value = serde_json::from_slice(&output.stdout).ok()?;
    report(&json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use onyx_core::health::Verdict;
    use onyx_core::{Bus, DriveId};

    /// The JSON of `smartctl --json -a disk0` on an APPLE SSD AP0512Z, cut
    /// down to the keys that Onyx reads, captured on 2026-10-10.
    const APPLE_SSD: &str = r#"{"model_name": "APPLE SSD AP0512Z", "serial_number": "05b171ac3e29a02e", "firmware_version": "241.0.12", "smart_status": {"passed": true, "nvme": {"value": 0}}, "nvme_smart_health_information_log": {"nsid": -1, "critical_warning": 0, "temperature": 33, "available_spare": 100, "available_spare_threshold": 99, "percentage_used": 0, "data_units_read": 35161000, "data_units_written": 16954057, "host_reads": 768906324, "host_writes": 235695719, "controller_busy_time": 0, "power_cycles": 195, "power_on_hours": 111, "unsafe_shutdowns": 11, "media_errors": 0, "num_err_log_entries": 0}, "temperature": {"current": 33}, "power_on_time": {"hours": 111}, "power_cycle_count": 195, "device": {"name": "disk0", "info_name": "disk0", "type": "nvme", "protocol": "NVMe"}}"#;

    #[test]
    fn the_json_of_a_real_nvme_drive_gives_the_same_report() {
        let json: Value = serde_json::from_str(APPLE_SSD).unwrap();
        let r = report(&json).unwrap();
        assert_eq!(r.protocol(), "NVMe");
        assert_eq!(
            r.identity(),
            (
                Some("APPLE SSD AP0512Z"),
                Some("05b171ac3e29a02e"),
                Some("241.0.12")
            )
        );
        assert_eq!(r.temperature_celsius(), Some(33));
        assert_eq!(r.power_on_hours(), Some(111));
        assert_eq!(r.power_cycles(), Some(195));
        assert_eq!(r.assessment().verdict, Verdict::Good);
        let Report::Nvme { log, .. } = r else {
            panic!("not an NVMe report");
        };
        assert_eq!(log.available_spare_threshold, 99);
        assert_eq!(log.data_units_read, 35_161_000);
        assert_eq!(log.unsafe_shutdowns, 11);
    }

    /// The shape of the ATA part of the JSON, from the JSON schema of
    /// smartmontools. No SATA drive was at hand to capture one.
    const ATA_DRIVE: &str = r#"{"model_name": "Samsung SSD 860 EVO 1TB", "serial_number": "S4EWNX0R123456", "firmware_version": "RVT04B6Q",
        "ata_smart_attributes": {"revision": 1, "table": [
            {"id": 5, "name": "Reallocated_Sector_Ct", "value": 100, "worst": 100, "thresh": 10, "flags": {"value": 51}, "raw": {"value": 3, "string": "3"}},
            {"id": 9, "name": "Power_On_Hours", "value": 95, "worst": 95, "thresh": 0, "flags": {"value": 50}, "raw": {"value": 21544, "string": "21544"}},
            {"id": 194, "name": "Temperature_Celsius", "value": 64, "worst": 48, "thresh": 0, "flags": {"value": 34}, "raw": {"value": 223339610148, "string": "36 (Min/Max 20/52)"}}
        ]}}"#;

    #[test]
    fn the_json_of_an_ata_drive_gives_each_attribute() {
        let json: Value = serde_json::from_str(ATA_DRIVE).unwrap();
        let r = report(&json).unwrap();
        assert_eq!(r.protocol(), "ATA");
        assert_eq!(r.power_on_hours(), Some(21_544));
        assert_eq!(r.temperature_celsius(), Some(36));
        // Three reallocated sectors are a caution.
        assert_eq!(r.assessment().verdict, Verdict::Caution);
        let Report::Ata { smart, .. } = r else {
            panic!("not an ATA report");
        };
        let a = smart.attribute(5).unwrap();
        assert_eq!((a.current, a.worst, a.threshold), (100, 100, Some(10)));
        assert!(a.pre_fail());
    }

    #[test]
    fn json_with_no_health_data_gives_no_report() {
        let json: Value = serde_json::from_str(
            r#"{"model_name": "Msft Virtual Disk", "smart_support": {"available": false}}"#,
        )
        .unwrap();
        assert!(report(&json).is_none());
    }

    #[test]
    fn json_with_values_of_the_wrong_kind_gives_zeros_and_not_a_panic() {
        let json: Value = serde_json::from_str(
            r#"{"nvme_smart_health_information_log": {"temperature": "hot", "power_cycles": -4}}"#,
        )
        .unwrap();
        let r = report(&json).unwrap();
        assert_eq!(r.temperature_celsius(), None);
        assert_eq!(r.power_cycles(), Some(0));
    }

    #[test]
    fn each_host_gives_smartctl_the_name_that_it_takes() {
        let mut d = DriveInfo::new(DriveId::new("2"), r"\\.\PhysicalDrive2", "a drive");
        d.bus = Bus::Usb;
        assert_eq!(device_name(Host::Windows, &d), "/dev/pd2");
        let d = DriveInfo::new(DriveId::new("disk4"), "/dev/rdisk4", "a drive");
        assert_eq!(device_name(Host::Macos, &d), "disk4");
        let d = DriveInfo::new(DriveId::new("sdb"), "/dev/sdb", "a drive");
        assert_eq!(device_name(Host::Linux, &d), "/dev/sdb");
    }
}
