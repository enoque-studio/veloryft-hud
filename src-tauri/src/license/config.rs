use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::client::WebPkiServerVerifier;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, Error, RootCertStore, SignatureScheme,
};
use sha2::{Digest, Sha256};
use url::Url;

use super::{is_lower_hex, LicenseError, TrustedResponseKey};

mod embedded {
    include!(concat!(env!("OUT_DIR"), "/license_config.rs"));
}

const MAX_LICENSE_RESPONSE_BYTES: usize = 64 * 1024;

pub(super) struct LicenseConfig {
    origin: Url,
    pub(super) app_hmac_secret: Vec<u8>,
    pub(super) trusted_response_keys: Vec<TrustedResponseKey>,
    spki_pins: Vec<[u8; 32]>,
}

impl LicenseConfig {
    pub(super) fn from_embedded() -> Result<Self, LicenseError> {
        let origin = embedded::LICENSE_ORIGIN.ok_or(LicenseError::NotProvisioned)?;
        let app_hmac_secret =
            embedded::LICENSE_APP_HMAC_SECRET.ok_or(LicenseError::NotProvisioned)?;
        Self::from_parts(
            origin,
            app_hmac_secret,
            embedded::LICENSE_RESPONSE_PUBLIC_KEYS,
            embedded::LICENSE_SPKI_PINS,
        )
    }

    fn from_parts(
        origin: &str,
        app_hmac_secret: &str,
        response_public_keys: &[&str],
        spki_pins: &[&str],
    ) -> Result<Self, LicenseError> {
        let mut origin = Url::parse(origin).map_err(|_| LicenseError::InvalidConfiguration)?;
        if origin.scheme() != "https"
            || origin.host_str().is_none()
            || !origin.username().is_empty()
            || origin.password().is_some()
            || origin.query().is_some()
            || origin.fragment().is_some()
            || origin.path() != "/"
        {
            return Err(LicenseError::InvalidConfiguration);
        }
        origin.set_path("/");

        if app_hmac_secret.len() < 32 || response_public_keys.len() < 2 || spki_pins.len() < 2 {
            return Err(LicenseError::InvalidConfiguration);
        }

        let mut trusted_response_keys = Vec::with_capacity(response_public_keys.len());
        for encoded_key in response_public_keys {
            let key_bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded_key)
                .or_else(|_| base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(encoded_key))
                .map_err(|_| LicenseError::InvalidConfiguration)?;
            let key_bytes: [u8; 32] = key_bytes
                .as_slice()
                .try_into()
                .map_err(|_| LicenseError::InvalidConfiguration)?;
            let key = ed25519_dalek::VerifyingKey::from_bytes(&key_bytes)
                .map_err(|_| LicenseError::InvalidConfiguration)?;
            let trusted_key = TrustedResponseKey::from_key(key);
            if trusted_response_keys
                .iter()
                .any(|known: &TrustedResponseKey| known.key_id == trusted_key.key_id)
            {
                return Err(LicenseError::InvalidConfiguration);
            }
            trusted_response_keys.push(trusted_key);
        }

        let mut decoded_pins = Vec::with_capacity(spki_pins.len());
        for encoded_pin in spki_pins {
            if !is_lower_hex(encoded_pin, 64) {
                return Err(LicenseError::InvalidConfiguration);
            }
            let pin: [u8; 32] = hex::decode(encoded_pin)
                .map_err(|_| LicenseError::InvalidConfiguration)?
                .as_slice()
                .try_into()
                .map_err(|_| LicenseError::InvalidConfiguration)?;
            if decoded_pins.contains(&pin) {
                return Err(LicenseError::InvalidConfiguration);
            }
            decoded_pins.push(pin);
        }

        Ok(Self {
            origin,
            app_hmac_secret: app_hmac_secret.as_bytes().to_vec(),
            trusted_response_keys,
            spki_pins: decoded_pins,
        })
    }

    pub(super) fn endpoint(&self, path: &str) -> Result<Url, LicenseError> {
        if !matches!(
            path,
            "/v1/trial/start" | "/v1/license/activate" | "/v1/validate"
        ) {
            return Err(LicenseError::InvalidRequest);
        }
        self.origin
            .join(path)
            .map_err(|_| LicenseError::InvalidConfiguration)
    }

    pub(super) fn http_client(&self) -> Result<reqwest::Client, LicenseError> {
        let mut root_store = RootCertStore::empty();
        root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let webpki = WebPkiServerVerifier::builder(Arc::new(root_store.clone()))
            .build()
            .map_err(|_| LicenseError::InvalidConfiguration)?;

        let mut tls_config = ClientConfig::builder()
            .with_root_certificates(root_store)
            .with_no_client_auth();
        tls_config
            .dangerous()
            .set_certificate_verifier(Arc::new(PinnedServerVerifier {
                webpki,
                spki_pins: self.spki_pins.clone(),
            }));

        reqwest::Client::builder()
            .use_preconfigured_tls(tls_config)
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .user_agent(concat!("Veloryft/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| LicenseError::InvalidConfiguration)
    }
}

#[derive(Debug)]
struct PinnedServerVerifier {
    webpki: Arc<dyn ServerCertVerifier>,
    spki_pins: Vec<[u8; 32]>,
}

impl ServerCertVerifier for PinnedServerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        let verified = self.webpki.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        )?;
        let (_, certificate) = x509_parser::parse_x509_certificate(end_entity.as_ref())
            .map_err(|_| Error::InvalidCertificate(CertificateError::BadEncoding))?;
        let actual_pin: [u8; 32] =
            Sha256::digest(certificate.tbs_certificate.subject_pki.raw).into();
        if !self.spki_pins.contains(&actual_pin) {
            return Err(Error::General("SPKI pin mismatch".into()));
        }
        Ok(verified)
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signed: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.webpki
            .verify_tls12_signature(message, certificate, signed)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        certificate: &CertificateDer<'_>,
        signed: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.webpki
            .verify_tls13_signature(message, certificate, signed)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.webpki.supported_verify_schemes()
    }
}

pub(super) const fn max_license_response_bytes() -> usize {
    MAX_LICENSE_RESPONSE_BYTES
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    fn public_key(seed: u8) -> String {
        let signing_key = SigningKey::from_bytes(&[seed; 32]);
        base64::engine::general_purpose::STANDARD.encode(signing_key.verifying_key().as_bytes())
    }

    #[test]
    fn configuration_requires_https_and_two_rotation_values() {
        let public_keys = [public_key(1), public_key(2)];
        let pins = [hex::encode([1_u8; 32]), hex::encode([2_u8; 32])];
        let public_key_refs = [public_keys[0].as_str(), public_keys[1].as_str()];
        let pin_refs = [pins[0].as_str(), pins[1].as_str()];

        assert!(LicenseConfig::from_parts(
            "https://license.veloryft.com",
            "a provisioned HMAC secret with enough entropy",
            &public_key_refs,
            &pin_refs,
        )
        .is_ok());
        assert!(LicenseConfig::from_parts(
            "http://license.veloryft.com",
            "a provisioned HMAC secret with enough entropy",
            &public_key_refs,
            &pin_refs,
        )
        .is_err());
        assert!(LicenseConfig::from_parts(
            "https://license.veloryft.com",
            "a provisioned HMAC secret with enough entropy",
            &public_key_refs[..1],
            &pin_refs,
        )
        .is_err());
    }
}
