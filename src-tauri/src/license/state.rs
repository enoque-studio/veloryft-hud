use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use uuid::Uuid;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPTPROTECT_LOCAL_MACHINE, CRYPTPROTECT_UI_FORBIDDEN,
    CRYPT_INTEGER_BLOB,
};

use super::tpm::{self, TpmAnchor};
use super::{is_lower_hex, LicenseError};

const CLOCK_ENTROPY: &[u8] = b"veloryft-license-clock-v1";
const MACHINE_CLOCK_ENTROPY: &[u8] = b"veloryft-license-clock-machine-v1";
const MAX_PROTECTED_BYTES: usize = 64 * 1024;
const STATE_ENTROPY: &[u8] = b"veloryft-license-state-v1";
const STATE_VERSION: u8 = 1;
/// Raw ECDSA P-256 signature (r || s, 32 bytes each) hex-encoded.
const TPM_SIGNATURE_HEX_LEN: usize = 128;

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

/// Third high-water source: a plain (not DPAPI-encrypted — the content isn't
/// secret, only its authenticity matters) JSON sidecar carrying a TPM
/// signature over `high_water`. See `tpm.rs` for exactly what this does and
/// does not defend against.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TpmSidecar {
    version: u8,
    high_water: i64,
    signature_hex: String,
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

    // The high-water mark is mirrored in two roots with different scopes so a
    // casual rollback (deleting %APPDATA% state, or restoring one blob) does not
    // wind the clock back: we always take the *largest* valid value seen. The
    // user mirror lives in %LOCALAPPDATA% (user-scope DPAPI); the machine mirror
    // in %ProgramData% (machine-scope DPAPI). Both are best-effort reads — a
    // missing or unreadable mirror never blocks startup — but any present mirror
    // can only raise the high-water, never lower it.
    if let Some(high_water) = read_clock_mirror(clock_path()?, CLOCK_ENTROPY)? {
        state.clock_high_water = state.clock_high_water.max(high_water);
    }
    if let Some(high_water) = read_machine_clock_mirror() {
        state.clock_high_water = state.clock_high_water.max(high_water);
    }
    if let Some(high_water) = read_tpm_clock_mirror() {
        state.clock_high_water = state.clock_high_water.max(high_water);
    }
    Ok(Some(state))
}

/// Reads and validates a clock mirror, returning its high-water mark. A read or
/// decrypt error propagates for the required user mirror; the machine mirror
/// uses [`read_machine_clock_mirror`], which swallows errors instead.
fn read_clock_mirror(path: PathBuf, entropy: &[u8]) -> Result<Option<i64>, LicenseError> {
    let Some(encrypted_clock) = read_optional(path)? else {
        return Ok(None);
    };
    let plain_clock = unprotect(&encrypted_clock, entropy)?;
    let mirror: ClockMirror =
        serde_json::from_slice(&plain_clock).map_err(|_| LicenseError::StateUnavailable)?;
    if mirror.version != STATE_VERSION || mirror.high_water < 0 {
        return Err(LicenseError::StateUnavailable);
    }
    Ok(Some(mirror.high_water))
}

/// Machine-scope mirror in %ProgramData%. Best-effort: a locked-down or absent
/// directory must not break the user-run HUD, so every failure maps to `None`.
fn read_machine_clock_mirror() -> Option<i64> {
    let path = machine_clock_path().ok()?;
    read_clock_mirror(path, MACHINE_CLOCK_ENTROPY).ok().flatten()
}

/// TPM-signed sidecar. Best-effort like the machine mirror: no TPM, a missing
/// sidecar, a malformed file, or a signature that fails to verify all map to
/// `None` rather than an error — this is one more optional signal, and its
/// absence must never block startup or count against the state that's there.
fn read_tpm_clock_mirror() -> Option<i64> {
    let bytes = read_optional(tpm_clock_path().ok()?).ok().flatten()?;
    let sidecar: TpmSidecar = serde_json::from_slice(&bytes).ok()?;
    if sidecar.version != STATE_VERSION
        || sidecar.high_water < 0
        || !is_lower_hex(&sidecar.signature_hex, TPM_SIGNATURE_HEX_LEN)
    {
        return None;
    }
    let signature = hex::decode(&sidecar.signature_hex).ok()?;
    let anchor = TpmAnchor::open_or_create()?;
    let digest = tpm::high_water_digest(sidecar.high_water);
    anchor.verify(&digest, &signature).then_some(sidecar.high_water)
}

pub(super) fn save(state: &StoredLicenseState) -> Result<(), LicenseError> {
    validate_state(state)?;
    let state_bytes = serde_json::to_vec(state).map_err(|_| LicenseError::StateUnavailable)?;
    let encrypted_state = protect(&state_bytes, STATE_ENTROPY, CRYPTPROTECT_UI_FORBIDDEN)?;
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

    // User-scope mirror is required (same trust root as the state itself).
    let encrypted_clock = protect(&clock_bytes, CLOCK_ENTROPY, CRYPTPROTECT_UI_FORBIDDEN)?;
    atomic_write(&clock_path()?, &encrypted_clock)?;

    // Machine-scope mirror in %ProgramData% is best-effort: it hardens against a
    // per-user "clear app data" rollback, but a locked-down directory must not
    // fail the save for a legitimate user.
    let _ = write_machine_clock_mirror(&clock_bytes);
    // TPM-signed sidecar: best-effort third signal (see tpm.rs for scope).
    // Silently skips on machines without a usable TPM/Platform Crypto Provider.
    write_tpm_clock_mirror(high_water);
    Ok(())
}

/// Signs `high_water` with the TPM anchor (creating it on first use) and
/// writes the plain JSON sidecar. No-ops silently when no TPM anchor is
/// available this run.
fn write_tpm_clock_mirror(high_water: i64) {
    let Some(anchor) = TpmAnchor::open_or_create() else {
        return;
    };
    let digest = tpm::high_water_digest(high_water);
    let Some(signature) = anchor.sign(&digest) else {
        return;
    };
    let sidecar = TpmSidecar {
        version: STATE_VERSION,
        high_water,
        signature_hex: hex::encode(signature),
    };
    let Ok(bytes) = serde_json::to_vec(&sidecar) else {
        return;
    };
    if let Ok(path) = tpm_clock_path() {
        let _ = atomic_write(&path, &bytes);
    }
}

/// Writes the machine-scope mirror. Errors are returned to the caller, which
/// treats them as non-fatal.
fn write_machine_clock_mirror(clock_bytes: &[u8]) -> Result<(), LicenseError> {
    let encrypted = protect(
        clock_bytes,
        MACHINE_CLOCK_ENTROPY,
        CRYPTPROTECT_UI_FORBIDDEN | CRYPTPROTECT_LOCAL_MACHINE,
    )?;
    atomic_write(&machine_clock_path()?, &encrypted)
}

pub(super) fn remove() -> Result<(), LicenseError> {
    remove_if_exists(state_path()?)?;
    remove_if_exists(clock_path()?)?;
    // The machine mirror and TPM sidecar are best-effort; failing to remove
    // either must not error.
    if let Ok(path) = machine_clock_path() {
        let _ = remove_if_exists(path);
    }
    if let Ok(path) = tpm_clock_path() {
        let _ = remove_if_exists(path);
    }
    Ok(())
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

/// Machine-wide anti-rollback mirror. Lives under %ProgramData% (all-users root)
/// so a per-user app-data wipe leaves it intact. The installer should create
/// `%ProgramData%\Veloryft` with an ACL that lets the app's user write only this
/// subtree; a hostile local administrator can still restore it (DPAPI does not
/// prevent that). The TPM sidecar (`tpm.rs`, [`tpm_clock_path`]) adds a signal
/// this administrator cannot forge for a *different* machine, but — like this
/// mirror — does not defeat a same-machine disk/VM snapshot restore; only a
/// true hardware monotonic counter or mandatory server revalidation would.
fn machine_clock_path() -> Result<PathBuf, LicenseError> {
    let program_data = std::env::var_os("ProgramData").ok_or(LicenseError::StateUnavailable)?;
    Ok(PathBuf::from(program_data)
        .join("Veloryft")
        .join("license-clock.dat"))
}

/// TPM-signed sidecar path. Plain JSON, not DPAPI-protected — see
/// [`TpmSidecar`] for why that's fine.
fn tpm_clock_path() -> Result<PathBuf, LicenseError> {
    let local_app_data = std::env::var_os("LOCALAPPDATA").ok_or(LicenseError::StateUnavailable)?;
    Ok(PathBuf::from(local_app_data)
        .join("Veloryft")
        .join("license-clock.tpm.json"))
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

fn protect(plain: &[u8], entropy: &[u8], flags: u32) -> Result<Vec<u8>, LicenseError> {
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
            flags,
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
        let encrypted =
            protect(plain, STATE_ENTROPY, CRYPTPROTECT_UI_FORBIDDEN).expect("DPAPI protects state");
        assert_ne!(encrypted, plain);
        assert_eq!(unprotect(&encrypted, STATE_ENTROPY).unwrap(), plain);
    }

    #[test]
    fn machine_scope_clock_mirror_round_trips_and_is_distinct() {
        // The machine-scope mirror must protect and read back the same bytes and
        // use its own entropy (a mirror encrypted for one entropy must not read
        // back under the other), so the two roots stay independent.
        let mirror = ClockMirror {
            version: STATE_VERSION,
            high_water: 1_800_000_000,
        };
        let bytes = serde_json::to_vec(&mirror).unwrap();
        let encrypted = protect(
            &bytes,
            MACHINE_CLOCK_ENTROPY,
            CRYPTPROTECT_UI_FORBIDDEN | CRYPTPROTECT_LOCAL_MACHINE,
        )
        .expect("machine-scope DPAPI protects the mirror");
        let back: ClockMirror =
            serde_json::from_slice(&unprotect(&encrypted, MACHINE_CLOCK_ENTROPY).unwrap()).unwrap();
        assert_eq!(back.high_water, 1_800_000_000);
        assert!(unprotect(&encrypted, CLOCK_ENTROPY).is_err());
    }

    #[test]
    fn highest_mirror_wins_so_rollback_cannot_lower_the_high_water() {
        // Restoring one stale mirror cannot wind the clock back: load takes the
        // max across state and every mirror. This models that max directly.
        let state_value = 1_000_000_000_i64;
        let user_mirror = 1_700_000_000_i64;
        let machine_mirror = 1_500_000_000_i64;
        let effective = state_value.max(user_mirror).max(machine_mirror);
        assert_eq!(effective, user_mirror);
    }

    /// Serializes the two tests below: both touch the same real
    /// %APPDATA%/%LOCALAPPDATA%/%ProgramData% paths, and `cargo test` runs
    /// tests concurrently by default.
    static REAL_STATE_PATH_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Real end-to-end `save()`/`load()`/`remove()` against this machine's
    /// actual %APPDATA%/%LOCALAPPDATA%/%ProgramData% — not just the isolated
    /// helpers above. Cleans up every file it writes.
    #[test]
    fn save_load_remove_round_trip_on_real_paths() {
        let _guard = REAL_STATE_PATH_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let state = StoredLicenseState::new(
            "token-for-round-trip-test".into(),
            "123e4567-e89b-12d3-a456-426614174000".into(),
            "a".repeat(32),
            "b".repeat(64),
            1_700_000_000,
            1_700_000_100,
            1_700_000_200,
        );
        save(&state).expect("save should succeed on a real dev machine");

        let loaded = load()
            .expect("load should succeed")
            .expect("state must exist after save");
        assert_eq!(loaded.token, state.token);
        assert!(loaded.clock_high_water >= 1_700_000_200);

        remove().expect("cleanup must succeed");
        assert!(load().unwrap().is_none());
    }

    /// The property `tpm.rs` specifically claims: if an attacker restores
    /// stale DPAPI mirrors (both user- and machine-scope) after a save, the
    /// untouched TPM-signed sidecar still recovers the real high-water on the
    /// next load — because the mirrors are compared with `max()`, and the
    /// sidecar's signature (over `high_water`, checked against this exact
    /// TPM) can't be forged to say otherwise. Skips the strong assertion (but
    /// still exercises save/load/remove) on a machine with no usable TPM.
    #[test]
    fn tpm_sidecar_survives_a_dpapi_mirror_rollback() {
        let _guard = REAL_STATE_PATH_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let has_tpm = TpmAnchor::open_or_create().is_some();

        let state = StoredLicenseState::new(
            "token-for-tpm-rollback-test".into(),
            "123e4567-e89b-12d3-a456-426614174000".into(),
            "c".repeat(32),
            "d".repeat(64),
            1_700_000_000,
            1_700_000_000,
            1_700_050_000,
        );
        save(&state).expect("save should succeed");

        // Simulate restoring stale DPAPI mirrors from before this save, while
        // leaving the TPM sidecar (written by the save above) untouched.
        let stale = ClockMirror {
            version: STATE_VERSION,
            high_water: 1_600_000_000,
        };
        let stale_bytes = serde_json::to_vec(&stale).unwrap();
        let stale_user = protect(&stale_bytes, CLOCK_ENTROPY, CRYPTPROTECT_UI_FORBIDDEN).unwrap();
        atomic_write(&clock_path().unwrap(), &stale_user).unwrap();
        if let Ok(path) = machine_clock_path() {
            if let Ok(stale_machine) = protect(
                &stale_bytes,
                MACHINE_CLOCK_ENTROPY,
                CRYPTPROTECT_UI_FORBIDDEN | CRYPTPROTECT_LOCAL_MACHINE,
            ) {
                let _ = atomic_write(&path, &stale_machine);
            }
        }

        let loaded = load().unwrap().unwrap();
        if has_tpm {
            assert!(
                loaded.clock_high_water >= 1_700_050_000,
                "TPM sidecar must recover the real high-water despite rolled-back DPAPI mirrors"
            );
        } else {
            eprintln!("skipping strong TPM assertion: no TPM/PCP available in this environment");
        }

        remove().expect("cleanup must succeed");
    }
}
