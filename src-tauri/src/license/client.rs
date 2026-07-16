use std::time::{SystemTime, UNIX_EPOCH};

use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

use super::config::{max_license_response_bytes, LicenseConfig};
use super::fingerprint::{self, LocalFingerprint, MachineInfo};
use super::state::{self, StoredLicenseState};
use super::{attest_request, verify_response, verify_token, LicenseError, SignedResponseHeaders};

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct LicenseAccess {
    pub plan: String,
    pub offline: bool,
}

struct AuthenticatedResponse {
    status: u16,
    server_timestamp: i64,
    body: Vec<u8>,
}

#[derive(Serialize)]
struct EnrollmentRequest<'a> {
    fingerprint: &'a [fingerprint::FingerprintComponent],
    machine: &'a MachineInfo,
    app_version: &'static str,
}

#[derive(Serialize)]
struct ActivationRequest<'a> {
    key: &'a str,
    fingerprint: &'a [fingerprint::FingerprintComponent],
    machine: &'a MachineInfo,
    app_version: &'static str,
}

#[derive(Serialize)]
struct ValidationRequest<'a> {
    fingerprint: &'a [fingerprint::FingerprintComponent],
    machine: &'a MachineInfo,
    app_version: &'static str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EntitlementResponse {
    status: String,
    plan: String,
    #[serde(default)]
    token: Option<String>,
    #[serde(rename = "expires_at")]
    _expires_at: String,
    #[serde(rename = "days_left")]
    _days_left: i64,
    #[serde(default, rename = "message")]
    _message: Option<String>,
    #[serde(default, rename = "upgrade_url")]
    _upgrade_url: Option<String>,
}

/// Performs the entitlement gate before the overlay opens. In release builds
/// the build script guarantees that a missing trust configuration is fatal.
pub fn authorize_startup() -> Result<LicenseAccess, LicenseError> {
    let activation_key = activation_key_from_process_arguments()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|_| LicenseError::NetworkUnavailable)?;
    runtime.block_on(authorize(activation_key))
}

async fn authorize(activation_key: Option<String>) -> Result<LicenseAccess, LicenseError> {
    let config = match LicenseConfig::from_embedded() {
        Ok(config) => config,
        Err(LicenseError::NotProvisioned) if cfg!(debug_assertions) => {
            return Ok(LicenseAccess {
                plan: "development".into(),
                offline: true,
            });
        }
        Err(error) => return Err(error),
    };
    let fingerprint = tokio::task::spawn_blocking(fingerprint::collect)
        .await
        .map_err(|_| LicenseError::FingerprintUnavailable)??;
    let now = unix_timestamp()?;
    let previous = state::load()?;
    let http = config.http_client()?;

    if let Some(key) = activation_key {
        let request = ActivationRequest {
            key: &key,
            fingerprint: &fingerprint.components,
            machine: &fingerprint.machine,
            app_version: APP_VERSION,
        };
        let response = post_signed(&http, &config, "/v1/license/activate", &request, None).await?;
        return accept_active(
            &config,
            &fingerprint,
            previous.as_ref(),
            now,
            response,
            false,
        );
    }

    match previous {
        Some(previous) => {
            match validate_existing(&http, &config, &fingerprint, &previous, now).await {
                Ok(access) => Ok(access),
                Err(error) if permits_offline_fallback(&error) => {
                    offline_access(&config, &fingerprint, &previous, now)
                }
                Err(error) => Err(error),
            }
        }
        None => {
            let request = EnrollmentRequest {
                fingerprint: &fingerprint.components,
                machine: &fingerprint.machine,
                app_version: APP_VERSION,
            };
            let response = post_signed(&http, &config, "/v1/trial/start", &request, None).await?;
            accept_active(&config, &fingerprint, None, now, response, false)
        }
    }
}

async fn validate_existing(
    http: &reqwest::Client,
    config: &LicenseConfig,
    fingerprint: &LocalFingerprint,
    previous: &StoredLicenseState,
    now: i64,
) -> Result<LicenseAccess, LicenseError> {
    let request = ValidationRequest {
        fingerprint: &fingerprint.components,
        machine: &fingerprint.machine,
        app_version: APP_VERSION,
    };
    let response = post_signed(
        http,
        config,
        "/v1/validate",
        &request,
        Some(&previous.token),
    )
    .await?;
    accept_active(config, fingerprint, Some(previous), now, response, false)
}

fn offline_access(
    config: &LicenseConfig,
    fingerprint: &LocalFingerprint,
    previous: &StoredLicenseState,
    now: i64,
) -> Result<LicenseAccess, LicenseError> {
    if previous.fingerprint_binding != fingerprint.binding || previous.clock_rolled_back(now) {
        return Err(LicenseError::NetworkUnavailable);
    }
    let claims = verify_token(
        &previous.token,
        &config.trusted_response_keys,
        previous.effective_now(now),
        previous.highest_iat,
        Some(&previous.fingerprint_tag),
        Some(&previous.machine_id),
    )?;
    Ok(LicenseAccess {
        plan: claims.plan,
        offline: true,
    })
}

fn accept_active(
    config: &LicenseConfig,
    fingerprint: &LocalFingerprint,
    previous: Option<&StoredLicenseState>,
    now: i64,
    response: AuthenticatedResponse,
    offline: bool,
) -> Result<LicenseAccess, LicenseError> {
    let mut entitlement = decode_entitlement(&response)?;
    if entitlement.status == "expired" {
        state::remove()?;
        return Err(LicenseError::ExpiredEntitlement);
    }

    let token = entitlement
        .token
        .take()
        .ok_or(LicenseError::EntitlementDenied)?;
    let effective_now = previous
        .map(|state| state.effective_now(now))
        .unwrap_or(now)
        .max(response.server_timestamp);
    let claims = verify_token(
        &token,
        &config.trusted_response_keys,
        effective_now,
        previous.map_or(0, |state| state.highest_iat),
        previous.map(|state| state.fingerprint_tag.as_str()),
        previous.map(|state| state.machine_id.as_str()),
    )?;
    if entitlement.plan != claims.plan {
        return Err(LicenseError::EntitlementDenied);
    }

    let state = StoredLicenseState::new(
        token,
        claims.mid.clone(),
        claims.fp,
        fingerprint.binding.clone(),
        previous.map_or(claims.iat, |state| state.highest_iat.max(claims.iat)),
        previous.map_or(response.server_timestamp, |state| {
            state.response_high_water.max(response.server_timestamp)
        }),
        previous.map_or(now.max(response.server_timestamp), |state| {
            state
                .clock_high_water
                .max(now)
                .max(response.server_timestamp)
        }),
    );
    state::save(&state)?;

    Ok(LicenseAccess {
        plan: claims.plan,
        offline,
    })
}

fn decode_entitlement(
    response: &AuthenticatedResponse,
) -> Result<EntitlementResponse, LicenseError> {
    if response.status != 200 {
        return Err(LicenseError::EntitlementDenied);
    }
    let entitlement: EntitlementResponse =
        serde_json::from_slice(&response.body).map_err(|_| LicenseError::EntitlementDenied)?;
    if !matches!(entitlement.status.as_str(), "active" | "expired")
        || !matches!(entitlement.plan.as_str(), "trial" | "monthly" | "annual" | "beta")
        || (entitlement.status == "active" && entitlement.token.is_none())
        || (entitlement.status == "expired" && entitlement.token.is_some())
    {
        return Err(LicenseError::EntitlementDenied);
    }
    Ok(entitlement)
}

async fn post_signed<T: Serialize>(
    http: &reqwest::Client,
    config: &LicenseConfig,
    path: &str,
    request_body: &T,
    bearer: Option<&str>,
) -> Result<AuthenticatedResponse, LicenseError> {
    let body = serde_json::to_vec(request_body).map_err(|_| LicenseError::InvalidRequest)?;
    if body.len() > 16 * 1024 {
        return Err(LicenseError::InvalidRequest);
    }
    let attestation = attest_request(
        "POST",
        path,
        unix_timestamp()?,
        &body,
        &config.app_hmac_secret,
    )?;

    let mut request = http
        .post(config.endpoint(path)?)
        .header(CONTENT_TYPE, "application/json")
        .header(ACCEPT, "application/json")
        .header("x-veloryft-timestamp", &attestation.timestamp)
        .header("x-veloryft-nonce", &attestation.nonce)
        .header("x-veloryft-signature", &attestation.signature)
        .body(body);
    if let Some(token) = bearer {
        let value = HeaderValue::from_str(&format!("Bearer {token}"))
            .map_err(|_| LicenseError::InvalidRequest)?;
        request = request.header(AUTHORIZATION, value);
    }

    let mut response = request
        .send()
        .await
        .map_err(|_| LicenseError::NetworkUnavailable)?;
    let status = response.status().as_u16();
    if response
        .content_length()
        .is_some_and(|size| size > max_license_response_bytes() as u64)
    {
        return Err(LicenseError::NetworkUnavailable);
    }
    let response_headers = response.headers().clone();
    let mut response_body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| LicenseError::NetworkUnavailable)?
    {
        let new_length = response_body
            .len()
            .checked_add(chunk.len())
            .ok_or(LicenseError::NetworkUnavailable)?;
        if new_length > max_license_response_bytes() {
            return Err(LicenseError::NetworkUnavailable);
        }
        response_body.extend_from_slice(&chunk);
    }

    let signed_headers = SignedResponseHeaders {
        timestamp: required_header(&response_headers, "x-veloryft-response-timestamp")?,
        body_sha256: required_header(&response_headers, "x-veloryft-response-body-sha256")?,
        key_id: required_header(&response_headers, "x-veloryft-key-id")?,
        signature: required_header(&response_headers, "x-veloryft-response-signature")?,
    };
    let server_timestamp = verify_response(
        status,
        &attestation.nonce,
        &signed_headers,
        &response_body,
        &config.trusted_response_keys,
    )?;
    Ok(AuthenticatedResponse {
        status,
        server_timestamp,
        body: response_body,
    })
}

fn required_header(headers: &HeaderMap, name: &str) -> Result<String, LicenseError> {
    headers
        .get(name)
        .ok_or(LicenseError::UnauthenticatedResponse)?
        .to_str()
        .map(str::to_owned)
        .map_err(|_| LicenseError::UnauthenticatedResponse)
}

fn permits_offline_fallback(error: &LicenseError) -> bool {
    matches!(
        error,
        LicenseError::NetworkUnavailable | LicenseError::UnauthenticatedResponse
    )
}

fn unix_timestamp() -> Result<i64, LicenseError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| LicenseError::NetworkUnavailable)?;
    i64::try_from(duration.as_secs()).map_err(|_| LicenseError::NetworkUnavailable)
}

fn activation_key_from_process_arguments() -> Result<Option<String>, LicenseError> {
    let arguments = std::env::args_os()
        .skip(1)
        .map(|argument| {
            argument
                .into_string()
                .map_err(|_| LicenseError::InvalidRequest)
        })
        .collect::<Result<Vec<_>, _>>()?;
    activation_key_from_arguments(&arguments)
}

fn activation_key_from_arguments(arguments: &[String]) -> Result<Option<String>, LicenseError> {
    let mut activation_key = None;
    let mut position = 0;
    while position < arguments.len() {
        if matches!(arguments[position].as_str(), "--activate" | "--license-key") {
            let key = arguments
                .get(position + 1)
                .filter(|key| is_license_key(key))
                .cloned()
                .ok_or(LicenseError::InvalidRequest)?;
            if activation_key.replace(key).is_some() {
                return Err(LicenseError::InvalidRequest);
            }
            position += 1;
        }
        position += 1;
    }
    Ok(activation_key)
}

fn is_license_key(value: &str) -> bool {
    let mut groups = value.split('-');
    if groups.next() != Some("VLRY") {
        return false;
    }
    let alphabet = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
    groups.by_ref().take(4).all(|group| {
        group.len() == 4 && group.chars().all(|character| alphabet.contains(character))
    }) && groups.next().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_explicit_well_formed_activation_key() {
        let arguments = vec!["--activate".into(), "VLRY-ABCD-EFGH-JKLM-NPQR".into()];
        assert_eq!(
            activation_key_from_arguments(&arguments).unwrap(),
            Some("VLRY-ABCD-EFGH-JKLM-NPQR".into())
        );
        assert!(activation_key_from_arguments(&["--activate".into(), "bad".into()]).is_err());
        assert!(!is_license_key("VLRY-ABCD-EFGH-JKLM-NPQR-XXXX"));
    }
}
