use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
};

use super::{is_lower_hex, LicenseError};

const CLOCK_ENTROPY: &[u8] = b"veloryft-license-clock-v1";
const MAX_PROTECTED_BYTES: usize = 64 * 1024;
const STATE_ENTROPY: &[u8] = b"veloryft-license-state-v1";
const STATE_VERSION: u8 = 1;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct StoredLicenseState {
    version: u8,
    pub(super) token: String,
    pub(super) machine_id: String,
    pub(super) fingerprint_tag: String,
    pub(super) fingerprint_binding: String,
    pub(super) highest_iat: i64,
    pub(super) response_high_water: i64,
    pub(super) clock_high_water: i64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ClockMirror {
    version: u8,
    high_water: i64,
}

impl StoredLicenseState {
    pub(super) fn new(
        token: String,
        machine_id: String,
        fingerprint_tag: String,
        fingerprint_binding: String,
        highest_iat: i64,
        response_high_water: i64,
        clock_high_water: i64,
    ) -> Self {
        Self {
            version: STATE_VERSION,
            token,
            machine_id,
            fingerprint_tag,
            fingerprint_binding,
            highest_iat,
            response_high_water,
            clock_high_water,
        }
    }

    pub(super) fn effective_now(&self, current_time: i64) -> i64 {
        current_time
            .max(self.response_high_water)
            .max(self.clock_high_water)
    }

    pub(super) fn clock_rolled_back(&self, current_time: i64) -> bool {
        current_time.saturating_add(300) < self.clock_high_water
    }
}

pub(super) fn load() -> Result<Option<StoredLicenseState>, LicenseError> {
    let Some(encrypted) = read_optional(state_path()?)? else {
        return Ok(None);
    };
    let plain = unprotect(&encrypted, STATE_ENTROPY)?;
    let mut state: StoredLicenseState =
        serde_json::from_slice(&plain).map_err(|_| LicenseError::StateUnavailable)?;
    validate_state(&state)?;

    if let Some(encrypted_clock) = read_optional(clock_path()?)? {
        let plain_clock = unprotect(&encrypted_clock, CLOCK_ENTROPY)?;
        let mirror: ClockMirror =
            serde_json::from_slice(&plain_clock).map_err(|_| LicenseError::StateUnavailable)?;
        if mirror.version != STATE_VERSION || mirror.high_water < 0 {
            return Err(LicenseError::StateUnavailable);
        }
        state.clock_high_water = state.clock_high_water.max(mirror.high_water);
    }
    Ok(Some(state))
}

pub(super) fn save(state: &StoredLicenseState) -> Result<(), LicenseError> {
    validate_state(state)?;
    let state_bytes = serde_json::to_vec(state).map_err(|_| LicenseError::StateUnavailable)?;
    let encrypted_state = protect(&state_bytes, STATE_ENTROPY)?;
    atomic_write(&state_path()?, &encrypted_state)?;

    let high_water = state
        .response_high_water
        .max(state.clock_high_water)
        .max(state.highest_iat);
    let mirror = ClockMirror {
        version: STATE_VERSION,
        high_water,
    };
    let clock_bytes = serde_json::to_vec(&mirror).map_err(|_| LicenseError::StateUnavailable)?;
    let encrypted_clock = protect(&clock_bytes, CLOCK_ENTROPY)?;
    atomic_write(&clock_path()?, &encrypted_clock)
}

pub(super) fn remove() -> Result<(), LicenseError> {
    remove_if_exists(state_path()?)?;
    remove_if_exists(clock_path()?)
}

fn validate_state(state: &StoredLicenseState) -> Result<(), LicenseError> {
    if state.version != STATE_VERSION
        || state.token.is_empty()
        || state.token.len() > 2_048
        || Uuid::parse_str(&state.machine_id).is_err()
        || !is_lower_hex(&state.fingerprint_tag, 32)
        || !is_lower_hex(&state.fingerprint_binding, 64)
        || state.highest_iat < 0
        || state.response_high_water < 0
        || state.clock_high_water < 0
    {
        return Err(LicenseError::StateUnavailable);
    }
    Ok(())
}

fn state_path() -> Result<PathBuf, LicenseError> {
    let app_data = std::env::var_os("APPDATA").ok_or(LicenseError::StateUnavailable)?;
    Ok(PathBuf::from(app_data).join("Veloryft").join("license.dat"))
}

fn clock_path() -> Result<PathBuf, LicenseError> {
    let local_app_data = std::env::var_os("LOCALAPPDATA").ok_or(LicenseError::StateUnavailable)?;
    Ok(PathBuf::from(local_app_data)
        .join("Veloryft")
        .join("license-clock.dat"))
}

fn read_optional(path: PathBuf) -> Result<Option<Vec<u8>>, LicenseError> {
    match fs::read(path) {
        Ok(value) if value.len() <= MAX_PROTECTED_BYTES => Ok(Some(value)),
        Ok(_) => Err(LicenseError::StateUnavailable),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(LicenseError::StateUnavailable),
    }
}

fn atomic_write(path: &PathBuf, contents: &[u8]) -> Result<(), LicenseError> {
    let directory = path.parent().ok_or(LicenseError::StateUnavailable)?;
    fs::create_dir_all(directory).map_err(|_| LicenseError::StateUnavailable)?;
    let temporary = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| LicenseError::StateUnavailable)?;
        file.write_all(contents)
            .map_err(|_| LicenseError::StateUnavailable)?;
        file.sync_all()
            .map_err(|_| LicenseError::StateUnavailable)?;
        fs::rename(&temporary, path).map_err(|_| LicenseError::StateUnavailable)
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn remove_if_exists(path: PathBuf) -> Result<(), LicenseError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(LicenseError::StateUnavailable),
    }
}

fn protect(plain: &[u8], entropy: &[u8]) -> Result<Vec<u8>, LicenseError> {
    if plain.is_empty() || plain.len() > MAX_PROTECTED_BYTES {
        return Err(LicenseError::StateUnavailable);
    }
    let input = blob_from_slice(plain)?;
    let entropy = blob_from_slice(entropy)?;
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptProtectData(
            &input,
            PCWSTR::null(),
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|_| LicenseError::StateUnavailable)?;
        take_blob(&mut output)
    }
}

fn unprotect(encrypted: &[u8], entropy: &[u8]) -> Result<Vec<u8>, LicenseError> {
    if encrypted.is_empty() || encrypted.len() > MAX_PROTECTED_BYTES {
        return Err(LicenseError::StateUnavailable);
    }
    let input = blob_from_slice(encrypted)?;
    let entropy = blob_from_slice(entropy)?;
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        CryptUnprotectData(
            &input,
            None,
            Some(&entropy),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|_| LicenseError::StateUnavailable)?;
        take_blob(&mut output)
    }
}

fn blob_from_slice(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB, LicenseError> {
    let length = u32::try_from(bytes.len()).map_err(|_| LicenseError::StateUnavailable)?;
    Ok(CRYPT_INTEGER_BLOB {
        cbData: length,
        pbData: bytes.as_ptr().cast_mut(),
    })
}

unsafe fn take_blob(blob: &mut CRYPT_INTEGER_BLOB) -> Result<Vec<u8>, LicenseError> {
    if blob.pbData.is_null() || blob.cbData as usize > MAX_PROTECTED_BYTES {
        if !blob.pbData.is_null() {
            let _ = LocalFree(Some(HLOCAL(blob.pbData.cast())));
        }
        return Err(LicenseError::StateUnavailable);
    }
    let value = std::slice::from_raw_parts(blob.pbData, blob.cbData as usize).to_vec();
    let _ = LocalFree(Some(HLOCAL(blob.pbData.cast())));
    blob.pbData = std::ptr::null_mut();
    blob.cbData = 0;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_time_never_moves_backwards() {
        let state = StoredLicenseState::new(
            "token".into(),
            "123e4567-e89b-12d3-a456-426614174000".into(),
            "a".repeat(32),
            "b".repeat(64),
            100,
            120,
            500,
        );
        assert_eq!(state.effective_now(90), 500);
        assert!(state.clock_rolled_back(90));
        assert!(!state.clock_rolled_back(499));
    }

    #[test]
    fn dpapi_round_trip_keeps_state_out_of_plaintext() {
        let plain = b"license-state-test";
        let encrypted = protect(plain, STATE_ENTROPY).expect("DPAPI protects state");
        assert_ne!(encrypted, plain);
        assert_eq!(unprotect(&encrypted, STATE_ENTROPY).unwrap(), plain);
    }
}
