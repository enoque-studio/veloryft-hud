//! Verificação local do contrato de licenças Veloryft.
//!
//! Este módulo não concede acesso com base em dados locais não autenticados:
//! cada resposta e token passam por assinatura Ed25519, e a requisição é
//! vinculada aos bytes exatos enviados por HMAC.

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use hmac::{Hmac, Mac};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

const RESPONSE_DOMAIN: &str = "VELORYFT-RESPONSE-V1";
const REQUEST_DOMAIN: &str = "VELORYFT-REQUEST-V1";
const TOKEN_AUDIENCE: &str = "veloryft-hud";
const TOKEN_ISSUER: &str = "veloryft-license";
const TOKEN_VERSION: u8 = 1;
const TOKEN_CLOCK_SKEW_SECS: i64 = 60;

#[cfg(windows)]
mod client;
#[cfg(windows)]
mod config;
#[cfg(windows)]
mod fingerprint;
#[cfg(windows)]
mod state;
#[cfg(windows)]
mod tpm;

#[cfg(windows)]
pub use client::{authorize_startup, LicenseAccess};

#[derive(Debug, thiserror::Error)]
pub enum LicenseError {
    #[error("license client is not provisioned")]
    NotProvisioned,
    #[error("license client configuration is invalid")]
    InvalidConfiguration,
    #[error("machine fingerprint is unavailable")]
    FingerprintUnavailable,
    #[error("protected license state is unavailable")]
    StateUnavailable,
    #[error("license service is unavailable")]
    NetworkUnavailable,
    #[error("license entitlement was denied")]
    EntitlementDenied,
    #[error("license entitlement has expired")]
    ExpiredEntitlement,
    #[error("license request is invalid")]
    InvalidRequest,
    #[error("license response is not authenticated")]
    UnauthenticatedResponse,
    #[error("license token is invalid")]
    InvalidToken,
    #[error("license token has expired")]
    ExpiredToken,
}

/// Request fields that must be sent together with the exact serialized body.
#[derive(Clone, PartialEq, Eq)]
pub struct RequestAttestation {
    pub timestamp: String,
    pub nonce: String,
    pub signature: String,
}

/// The signed-response headers, kept separate from HTTP so the cryptographic
/// contract can be tested without a network stack.
#[derive(Clone, PartialEq, Eq)]
pub struct SignedResponseHeaders {
    pub timestamp: String,
    pub body_sha256: String,
    pub key_id: String,
    pub signature: String,
}

#[derive(Clone)]
pub struct TrustedResponseKey {
    pub key_id: String,
    pub key: VerifyingKey,
}

impl TrustedResponseKey {
    pub fn from_key(key: VerifyingKey) -> Self {
        let key_id = hex::encode(&Sha256::digest(key.as_bytes())[..16]);
        Self { key_id, key }
    }
}

/// Signed entitlement token issued by the licensing service.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntitlementClaims {
    pub v: u8,
    pub iss: String,
    pub aud: String,
    pub kid: String,
    pub typ: String,
    pub plan: String,
    pub mid: String,
    #[serde(default)]
    pub lic: Option<String>,
    pub fp: String,
    pub iat: i64,
    pub exp: i64,
    pub sub_exp: i64,
    pub jti: String,
}

/// Creates a nonce and HMAC for a request. `body` must be sent unchanged.
pub fn attest_request(
    method: &str,
    path_and_query: &str,
    timestamp: i64,
    body: &[u8],
    app_hmac_secret: &[u8],
) -> Result<RequestAttestation, LicenseError> {
    if !is_canonical_timestamp(timestamp)
        || method.is_empty()
        || !path_and_query.starts_with('/')
        || app_hmac_secret.len() < 32
    {
        return Err(LicenseError::InvalidRequest);
    }

    let mut random_bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut random_bytes);
    let nonce = hex::encode(random_bytes);
    let timestamp = timestamp.to_string();
    let canonical = request_canonical(method, path_and_query, &timestamp, &nonce, body);
    let mut mac =
        HmacSha256::new_from_slice(app_hmac_secret).map_err(|_| LicenseError::InvalidRequest)?;
    mac.update(canonical.as_bytes());

    Ok(RequestAttestation {
        timestamp,
        nonce,
        signature: hex::encode(mac.finalize().into_bytes()),
    })
}

/// Verifies that the complete HTTP response was signed for this request nonce.
/// Returns the authenticated server timestamp on success.
pub fn verify_response(
    status: u16,
    request_nonce: &str,
    headers: &SignedResponseHeaders,
    body: &[u8],
    trusted_keys: &[TrustedResponseKey],
) -> Result<i64, LicenseError> {
    if !is_nonce(request_nonce)
        || !is_lower_hex(&headers.body_sha256, 64)
        || !is_lower_hex(&headers.key_id, 32)
    {
        return Err(LicenseError::UnauthenticatedResponse);
    }

    let timestamp = parse_canonical_timestamp(&headers.timestamp)
        .ok_or(LicenseError::UnauthenticatedResponse)?;
    let actual_hash = hex::encode(Sha256::digest(body));
    if actual_hash != headers.body_sha256 {
        return Err(LicenseError::UnauthenticatedResponse);
    }

    let trusted_key = trusted_keys
        .iter()
        .find(|candidate| candidate.key_id == headers.key_id)
        .ok_or(LicenseError::UnauthenticatedResponse)?;
    let expected_key_id = hex::encode(&Sha256::digest(trusted_key.key.as_bytes())[..16]);
    if expected_key_id != headers.key_id {
        return Err(LicenseError::UnauthenticatedResponse);
    }

    let signature_bytes = B64
        .decode(&headers.signature)
        .map_err(|_| LicenseError::UnauthenticatedResponse)?;
    if B64.encode(&signature_bytes) != headers.signature {
        return Err(LicenseError::UnauthenticatedResponse);
    }
    let signature = Signature::from_slice(&signature_bytes)
        .map_err(|_| LicenseError::UnauthenticatedResponse)?;
    let canonical = response_canonical(
        status,
        request_nonce,
        &headers.timestamp,
        &headers.key_id,
        &headers.body_sha256,
    );
    trusted_key
        .key
        .verify(canonical.as_bytes(), &signature)
        .map_err(|_| LicenseError::UnauthenticatedResponse)?;

    Ok(timestamp)
}

/// Validates every claim before an entitlement can enable the overlay.
pub fn verify_token(
    token: &str,
    trusted_keys: &[TrustedResponseKey],
    effective_now: i64,
    highest_accepted_iat: i64,
    expected_fingerprint_tag: Option<&str>,
    expected_machine_id: Option<&str>,
) -> Result<EntitlementClaims, LicenseError> {
    let (payload_text, signature_text) = token.split_once('.').ok_or(LicenseError::InvalidToken)?;
    if signature_text.contains('.') {
        return Err(LicenseError::InvalidToken);
    }

    let payload = B64
        .decode(payload_text)
        .map_err(|_| LicenseError::InvalidToken)?;
    if B64.encode(&payload) != payload_text {
        return Err(LicenseError::InvalidToken);
    }
    let claims: EntitlementClaims =
        serde_json::from_slice(&payload).map_err(|_| LicenseError::InvalidToken)?;

    let trusted_key = trusted_keys
        .iter()
        .find(|candidate| candidate.key_id == claims.kid)
        .ok_or(LicenseError::InvalidToken)?;
    let expected_key_id = hex::encode(&Sha256::digest(trusted_key.key.as_bytes())[..16]);
    if claims.kid != expected_key_id {
        return Err(LicenseError::InvalidToken);
    }

    let signature_bytes = B64
        .decode(signature_text)
        .map_err(|_| LicenseError::InvalidToken)?;
    if B64.encode(&signature_bytes) != signature_text {
        return Err(LicenseError::InvalidToken);
    }
    let signature =
        Signature::from_slice(&signature_bytes).map_err(|_| LicenseError::InvalidToken)?;
    trusted_key
        .key
        .verify(payload_text.as_bytes(), &signature)
        .map_err(|_| LicenseError::InvalidToken)?;

    if claims.v != TOKEN_VERSION
        || claims.iss != TOKEN_ISSUER
        || claims.aud != TOKEN_AUDIENCE
        || !matches!(claims.typ.as_str(), "trial" | "license")
        || !matches!(claims.plan.as_str(), "trial" | "monthly" | "annual")
        || (claims.typ == "trial" && (claims.plan != "trial" || claims.lic.is_some()))
        || (claims.typ == "license" && (claims.plan == "trial" || claims.lic.is_none()))
        || !is_lower_hex(&claims.fp, 32)
        || Uuid::parse_str(&claims.mid).is_err()
        || Uuid::parse_str(&claims.jti).is_err()
        || claims
            .lic
            .as_deref()
            .is_some_and(|license_id| Uuid::parse_str(license_id).is_err())
        || claims.iat <= 0
        || claims.iat > effective_now.saturating_add(TOKEN_CLOCK_SKEW_SECS)
        || claims.iat < highest_accepted_iat
        || claims.exp <= 0
        || claims.sub_exp <= 0
        || claims.exp > claims.sub_exp
    {
        return Err(LicenseError::InvalidToken);
    }
    if effective_now >= claims.exp || effective_now >= claims.sub_exp {
        return Err(LicenseError::ExpiredToken);
    }
    if expected_fingerprint_tag.is_some_and(|tag| tag != claims.fp)
        || expected_machine_id.is_some_and(|machine_id| machine_id != claims.mid)
    {
        return Err(LicenseError::InvalidToken);
    }

    Ok(claims)
}

fn request_canonical(
    method: &str,
    path_and_query: &str,
    timestamp: &str,
    nonce: &str,
    body: &[u8],
) -> String {
    let body_hash = hex::encode(Sha256::digest(body));
    format!("{REQUEST_DOMAIN}\n{method}\n{path_and_query}\n{timestamp}\n{nonce}\n{body_hash}")
}

fn response_canonical(
    status: u16,
    request_nonce: &str,
    timestamp: &str,
    key_id: &str,
    body_hash: &str,
) -> String {
    format!("{RESPONSE_DOMAIN}\n{status}\n{request_nonce}\n{timestamp}\n{key_id}\n{body_hash}")
}

fn is_canonical_timestamp(timestamp: i64) -> bool {
    timestamp > 0
}

fn parse_canonical_timestamp(value: &str) -> Option<i64> {
    let timestamp = value.parse::<i64>().ok()?;
    (is_canonical_timestamp(timestamp) && timestamp.to_string() == value).then_some(timestamp)
}

fn is_nonce(value: &str) -> bool {
    is_lower_hex(value, 32)
}

fn is_lower_hex(value: &str, expected_length: usize) -> bool {
    value.len() == expected_length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    const REQUEST_NONCE: &str = "00112233445566778899aabbccddeeff";

    fn signed_response(
        signing_key: &SigningKey,
        status: u16,
        request_nonce: &str,
        timestamp: &str,
        body: &[u8],
    ) -> SignedResponseHeaders {
        let key_id = hex::encode(&Sha256::digest(signing_key.verifying_key().as_bytes())[..16]);
        let body_sha256 = hex::encode(Sha256::digest(body));
        let canonical = response_canonical(status, request_nonce, timestamp, &key_id, &body_sha256);
        let signature = B64.encode(signing_key.sign(canonical.as_bytes()).to_bytes());
        SignedResponseHeaders {
            timestamp: timestamp.into(),
            body_sha256,
            key_id,
            signature,
        }
    }

    fn signed_token(signing_key: &SigningKey, claims: &EntitlementClaims) -> String {
        let payload_text = B64.encode(serde_json::to_vec(claims).unwrap());
        let signature = signing_key.sign(payload_text.as_bytes());
        format!("{payload_text}.{}", B64.encode(signature.to_bytes()))
    }

    fn sample_claims(signing_key: &SigningKey) -> EntitlementClaims {
        EntitlementClaims {
            v: TOKEN_VERSION,
            iss: TOKEN_ISSUER.into(),
            aud: TOKEN_AUDIENCE.into(),
            kid: hex::encode(&Sha256::digest(signing_key.verifying_key().as_bytes())[..16]),
            typ: "trial".into(),
            plan: "trial".into(),
            mid: "123e4567-e89b-12d3-a456-426614174000".into(),
            lic: None,
            fp: "0123456789abcdef0123456789abcdef".into(),
            iat: 1_700_000_000,
            exp: 1_700_010_000,
            sub_exp: 1_700_020_000,
            jti: "123e4567-e89b-12d3-a456-426614174001".into(),
        }
    }

    #[test]
    fn attestation_binds_the_exact_body() {
        let body = br#"{\"fingerprint\":[]}"#;
        let attestation = attest_request(
            "POST",
            "/v1/validate",
            1_700_000_000,
            body,
            b"this client HMAC secret is at least thirty-two bytes",
        )
        .unwrap();

        assert!(is_nonce(&attestation.nonce));
        let signed = request_canonical(
            "POST",
            "/v1/validate",
            &attestation.timestamp,
            &attestation.nonce,
            body,
        );
        let changed = request_canonical(
            "POST",
            "/v1/validate",
            &attestation.timestamp,
            &attestation.nonce,
            br#"{\"fingerprint\":[1]}"#,
        );
        assert_ne!(signed, changed);
    }

    #[test]
    fn signed_response_rejects_changed_body() {
        let signing_key = SigningKey::from_bytes(&[7_u8; 32]);
        let trusted_keys = vec![TrustedResponseKey::from_key(signing_key.verifying_key())];
        let body = br#"{\"status\":\"active\"}"#;
        let headers = signed_response(&signing_key, 200, REQUEST_NONCE, "1700000000", body);

        assert_eq!(
            verify_response(200, REQUEST_NONCE, &headers, body, &trusted_keys).unwrap(),
            1_700_000_000
        );
        assert!(verify_response(
            200,
            REQUEST_NONCE,
            &headers,
            br#"{\"status\":\"expired\"}"#,
            &trusted_keys,
        )
        .is_err());
    }

    #[test]
    fn token_requires_valid_signature_and_machine_binding() {
        let signing_key = SigningKey::from_bytes(&[8_u8; 32]);
        let trusted_keys = vec![TrustedResponseKey::from_key(signing_key.verifying_key())];
        let claims = sample_claims(&signing_key);
        let token = signed_token(&signing_key, &claims);

        assert!(verify_token(
            &token,
            &trusted_keys,
            1_700_000_001,
            0,
            Some(&claims.fp),
            Some(&claims.mid),
        )
        .is_ok());
        assert!(verify_token(
            &token,
            &trusted_keys,
            1_700_000_001,
            0,
            Some("ffffffffffffffffffffffffffffffff"),
            Some(&claims.mid),
        )
        .is_err());

        let tampered = format!("{token}x");
        assert!(verify_token(
            &tampered,
            &trusted_keys,
            1_700_000_001,
            0,
            Some(&claims.fp),
            Some(&claims.mid),
        )
        .is_err());
    }
}
