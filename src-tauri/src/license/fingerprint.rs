use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use wmi::WMIConnection;

use super::LicenseError;

const ANCHOR_KINDS: &[&str] = &["smbios_uuid", "baseboard_serial", "cpu_id"];
const MINIMUM_ANCHOR_SCORE: u8 = 5;

#[derive(Clone, Serialize)]
pub(super) struct FingerprintComponent {
    pub(super) kind: String,
    pub(super) hash: String,
}

#[derive(Clone, Serialize)]
pub(super) struct MachineInfo {
    pub(super) label: Option<String>,
    pub(super) os: Option<String>,
}

pub(super) struct LocalFingerprint {
    pub(super) components: Vec<FingerprintComponent>,
    pub(super) binding: String,
    pub(super) machine: MachineInfo,
}

#[derive(Deserialize)]
struct SystemProduct {
    #[serde(rename = "UUID")]
    uuid: Option<String>,
}

#[derive(Deserialize)]
struct Baseboard {
    #[serde(rename = "SerialNumber")]
    serial_number: Option<String>,
}

#[derive(Deserialize)]
struct Processor {
    #[serde(rename = "ProcessorId")]
    processor_id: Option<String>,
}

/// Runs only during entitlement startup, from a dedicated blocking thread.
/// WMI values never leave this module in raw form.
pub(super) fn collect() -> Result<LocalFingerprint, LicenseError> {
    let connection = WMIConnection::new().map_err(|_| LicenseError::FingerprintUnavailable)?;
    let systems: Vec<SystemProduct> = connection
        .raw_query("SELECT UUID FROM Win32_ComputerSystemProduct")
        .map_err(|_| LicenseError::FingerprintUnavailable)?;
    let baseboards: Vec<Baseboard> = connection
        .raw_query("SELECT SerialNumber FROM Win32_BaseBoard")
        .map_err(|_| LicenseError::FingerprintUnavailable)?;
    let processors: Vec<Processor> = connection
        .raw_query("SELECT ProcessorId FROM Win32_Processor")
        .map_err(|_| LicenseError::FingerprintUnavailable)?;

    let mut components = Vec::with_capacity(3);
    add_first_component(
        &mut components,
        "smbios_uuid",
        systems
            .iter()
            .filter_map(|system| system.uuid.as_deref())
            .find_map(normalize_identifier),
    );
    add_first_component(
        &mut components,
        "baseboard_serial",
        baseboards
            .iter()
            .filter_map(|baseboard| baseboard.serial_number.as_deref())
            .find_map(normalize_identifier),
    );
    add_first_component(
        &mut components,
        "cpu_id",
        processors
            .iter()
            .filter_map(|processor| processor.processor_id.as_deref())
            .find_map(normalize_identifier),
    );

    let anchor_score = components
        .iter()
        .map(|component| match component.kind.as_str() {
            "smbios_uuid" | "baseboard_serial" => 3,
            "cpu_id" => 2,
            _ => 0,
        })
        .sum::<u8>();
    if components.len() < 2 || anchor_score < MINIMUM_ANCHOR_SCORE {
        return Err(LicenseError::FingerprintUnavailable);
    }

    Ok(LocalFingerprint {
        binding: fingerprint_binding(&components),
        components,
        machine: MachineInfo {
            label: std::env::var("COMPUTERNAME")
                .ok()
                .and_then(|label| sanitize_machine_text(&label)),
            os: Some("Windows".into()),
        },
    })
}

fn add_first_component(
    components: &mut Vec<FingerprintComponent>,
    kind: &str,
    normalized_value: Option<String>,
) {
    let Some(value) = normalized_value else {
        return;
    };
    components.push(FingerprintComponent {
        kind: kind.into(),
        hash: hex::encode(Sha256::digest(value.as_bytes())),
    });
}

fn normalize_identifier(value: &str) -> Option<String> {
    let normalized = value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_uppercase();
    let placeholder = matches!(
        normalized.as_str(),
        "" | "NONE"
            | "UNKNOWN"
            | "DEFAULT STRING"
            | "SYSTEM SERIAL NUMBER"
            | "TO BE FILLED BY O.E.M."
            | "TO BE FILLED BY OEM"
    );
    let repeated = normalized
        .bytes()
        .next()
        .is_some_and(|first| normalized.bytes().all(|byte| byte == first));
    (!placeholder && !repeated && normalized.len() >= 3).then_some(normalized)
}

fn fingerprint_binding(components: &[FingerprintComponent]) -> String {
    let mut anchors = components
        .iter()
        .filter(|component| ANCHOR_KINDS.contains(&component.kind.as_str()))
        .map(|component| format!("{}:{}", component.kind, component.hash))
        .collect::<Vec<_>>();
    anchors.sort();

    let mut hasher = Sha256::new();
    hasher.update(b"VELORYFT-FINGERPRINT-BINDING-V1\n");
    for anchor in anchors {
        hasher.update(anchor.as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}

fn sanitize_machine_text(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_placeholders_and_makes_stable_binding() {
        assert_eq!(normalize_identifier("  abc  123 "), Some("ABC 123".into()));
        assert_eq!(normalize_identifier("To Be Filled By O.E.M."), None);
        assert_eq!(normalize_identifier("00000000"), None);

        let first = vec![
            FingerprintComponent {
                kind: "cpu_id".into(),
                hash: "b".repeat(64),
            },
            FingerprintComponent {
                kind: "smbios_uuid".into(),
                hash: "a".repeat(64),
            },
        ];
        let second = first.iter().cloned().rev().collect::<Vec<_>>();
        assert_eq!(fingerprint_binding(&first), fingerprint_binding(&second));
    }
}
