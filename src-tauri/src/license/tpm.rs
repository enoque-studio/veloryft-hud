//! TPM-backed anti-rollback anchor via CNG (Microsoft Platform Crypto Provider).
//!
//! A third, best-effort high-water source beyond the two DPAPI mirrors in
//! `state.rs`. A non-exportable ECDSA P-256 key, generated and held inside
//! the TPM (`NCryptCreatePersistedKey` with no export flag set — the private
//! key material never leaves the chip), signs the high-water mark before it
//! is written to a plain sidecar file.
//!
//! **What this defends against:** the state files (both DPAPI mirrors) being
//! copied to a different machine to look "fresh" there. The signature can
//! only be produced by this exact TPM, so a copied sidecar+mirror pair fails
//! verification anywhere else — a strictly stronger binding than DPAPI alone
//! provides, since DPAPI's guarantee is confidentiality/integrity, not
//! machine-uniqueness of the *signing* identity.
//!
//! **What this does NOT defend against:** a full disk/VM snapshot restored
//! onto the *same* physical machine. The TPM's persisted key is unaffected by
//! an OS-disk rollback, so a stale-but-genuinely-signed sidecar from an old
//! snapshot still verifies. Defeating that requires either a true
//! hardware-incremented monotonic counter (TPM2 NV counter via raw TBS
//! commands — a materially larger, harder-to-validate undertaking, and a
//! distinct future step) or, as this codebase already treats as the real
//! authority, mandatory periodic server revalidation
//! (`docs/CLIENT.md` §7, `SECURITY.md`).
//!
//! Every entry point here is best-effort: no TPM, PCP disabled in firmware, a
//! remote session without the chip, or any CNG failure must fall back to
//! "anchor unavailable" (`None`), never block the HUD from starting.

use sha2::{Digest, Sha256};
use windows::core::{w, PCWSTR};
use windows::Win32::Security::Cryptography::{
    NCryptCreatePersistedKey, NCryptFinalizeKey, NCryptFreeObject, NCryptOpenKey,
    NCryptOpenStorageProvider, NCryptSetProperty, NCryptSignHash, NCryptVerifySignature,
    CERT_KEY_SPEC, MS_PLATFORM_CRYPTO_PROVIDER, NCRYPT_ECDSA_P256_ALGORITHM,
    NCRYPT_EXPORT_POLICY_PROPERTY, NCRYPT_FLAGS, NCRYPT_KEY_HANDLE, NCRYPT_PROV_HANDLE,
    NCRYPT_SILENT_FLAG,
};

const KEY_NAME: PCWSTR = w!("VeloryftLicenseAnchor-v1");
/// `NCRYPT_EXPORT_POLICY_PROPERTY` value with every export bit clear.
const EXPORT_POLICY_NONE: [u8; 4] = 0u32.to_le_bytes();

pub struct TpmAnchor {
    provider: NCRYPT_PROV_HANDLE,
    key: NCRYPT_KEY_HANDLE,
}

impl Drop for TpmAnchor {
    fn drop(&mut self) {
        unsafe {
            let _ = NCryptFreeObject(self.key.into());
            let _ = NCryptFreeObject(self.provider.into());
        }
    }
}

impl TpmAnchor {
    /// Opens the persisted anchor key, creating it on first use. `None` means
    /// no TPM anchor is available this run (missing chip, disabled PCP,
    /// access denied, ...) — callers must treat that as "skip this signal",
    /// not as an error.
    pub fn open_or_create() -> Option<Self> {
        unsafe {
            let mut provider = NCRYPT_PROV_HANDLE::default();
            NCryptOpenStorageProvider(&mut provider, MS_PLATFORM_CRYPTO_PROVIDER, 0).ok()?;

            let mut key = NCRYPT_KEY_HANDLE::default();
            if NCryptOpenKey(
                provider,
                &mut key,
                KEY_NAME,
                CERT_KEY_SPEC(0),
                NCRYPT_SILENT_FLAG,
            )
            .is_ok()
            {
                return Some(Self { provider, key });
            }

            let mut key = NCRYPT_KEY_HANDLE::default();
            NCryptCreatePersistedKey(
                provider,
                &mut key,
                NCRYPT_ECDSA_P256_ALGORITHM,
                KEY_NAME,
                CERT_KEY_SPEC(0),
                NCRYPT_SILENT_FLAG,
            )
            .ok()?;

            // Belt-and-suspenders: the provider already defaults to
            // non-exportable, but state it explicitly so the key's identity
            // can never leave the TPM even if that default ever changes.
            let _ = NCryptSetProperty(
                key.into(),
                NCRYPT_EXPORT_POLICY_PROPERTY,
                &EXPORT_POLICY_NONE,
                NCRYPT_FLAGS(0),
            );

            if NCryptFinalizeKey(key, NCRYPT_SILENT_FLAG).is_err() {
                let _ = NCryptFreeObject(key.into());
                let _ = NCryptFreeObject(provider.into());
                return None;
            }
            Some(Self { provider, key })
        }
    }

    /// Signs a 32-byte SHA-256 digest, returning the raw (non-DER) ECDSA
    /// signature, or `None` on any CNG failure.
    pub fn sign(&self, digest: &[u8; 32]) -> Option<Vec<u8>> {
        unsafe {
            let mut needed: u32 = 0;
            NCryptSignHash(self.key, None, digest, None, &mut needed, NCRYPT_FLAGS(0)).ok()?;
            let mut signature = vec![0u8; needed as usize];
            let mut written: u32 = 0;
            NCryptSignHash(
                self.key,
                None,
                digest,
                Some(&mut signature),
                &mut written,
                NCRYPT_FLAGS(0),
            )
            .ok()?;
            signature.truncate(written as usize);
            Some(signature)
        }
    }

    /// Verifies a signature produced by [`Self::sign`] over the same digest.
    pub fn verify(&self, digest: &[u8; 32], signature: &[u8]) -> bool {
        unsafe {
            NCryptVerifySignature(self.key, None, digest, signature, NCRYPT_FLAGS(0)).is_ok()
        }
    }
}

/// Canonical digest for a high-water mark, domain-separated from any other
/// use of SHA-256 in this crate.
pub fn high_water_digest(high_water: i64) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"VELORYFT-TPM-ANCHOR-V1\n");
    hasher.update(high_water.to_le_bytes());
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Requires an actual TPM/PCP on the machine running the test; skips
    /// (prints and returns) rather than failing when unavailable, since CI
    /// and dev machines vary. Run manually on TPM-equipped hardware to get a
    /// real assertion; this still exercises the digest helper unconditionally.
    /// On TPM-equipped hardware this persists the real `KEY_NAME` key in the
    /// platform key store (the same one production uses) — intentional, not
    /// a leak to clean up; re-running the test reuses it via the open path.
    #[test]
    fn sign_verify_round_trip_and_tamper_detection() {
        let digest_a = high_water_digest(1_700_000_000);
        let digest_b = high_water_digest(1_700_000_001);
        assert_ne!(digest_a, digest_b);

        let Some(anchor) = TpmAnchor::open_or_create() else {
            eprintln!("skipping: no TPM/Platform Crypto Provider available in this environment");
            return;
        };
        let signature = anchor
            .sign(&digest_a)
            .expect("a just-opened TPM key must be able to sign");
        assert!(anchor.verify(&digest_a, &signature));
        assert!(
            !anchor.verify(&digest_b, &signature),
            "a signature over one high-water value must not verify another"
        );

        // Re-opening (the persisted-key path, not create) must yield a key
        // that verifies signatures made by the freshly-created one.
        let reopened = TpmAnchor::open_or_create().expect("anchor was just created");
        assert!(reopened.verify(&digest_a, &signature));
    }
}
