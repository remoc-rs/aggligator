//! TLS configurations for command line utilities.

use anyhow::{Context, Result};
use rustls::{
    ClientConfig, DigitallySignedStruct, RootCertStore, ServerConfig, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime},
};
use rustls_pemfile::{certs, private_key};
use std::{fs::File, io::BufReader, path::Path, sync::Arc};

static BUILTIN_CERT_PEM: &[u8] = include_bytes!("bin/agg-speed-cert.pem");
static BUILTIN_KEY_PEM: &[u8] = include_bytes!("bin/agg-speed-key.pem");

/// Server name of the built-in certificate.
pub const BUILTIN_SERVER_NAME: &str = "aggligator.rs";

/// Built-in self-signed certificate.
///
/// Its private key is public, thus it provides no authentication.
pub fn builtin_cert() -> CertificateDer<'static> {
    let mut reader = BufReader::new(BUILTIN_CERT_PEM);
    let mut certs = certs(&mut reader);
    certs.next().unwrap().unwrap()
}

/// Private key of the built-in self-signed certificate.
pub fn builtin_key() -> PrivateKeyDer<'static> {
    let mut reader = BufReader::new(BUILTIN_KEY_PEM);
    private_key(&mut reader).unwrap().unwrap()
}

/// Accepts every TLS server certificate.
///
/// Provides encryption without server authentication.
#[derive(Debug)]
pub struct NullVerifier;

impl ServerCertVerifier for NullVerifier {
    fn verify_server_cert(
        &self, _end_entity: &CertificateDer<'_>, _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>, _ocsp_response: &[u8], _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self, _message: &[u8], _cert: &CertificateDer<'_>, _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self, _message: &[u8], _cert: &CertificateDer<'_>, _dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![
            SignatureScheme::RSA_PKCS1_SHA1,
            SignatureScheme::ECDSA_SHA1_Legacy,
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP521_SHA512,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
            SignatureScheme::ED448,
        ]
    }
}

/// TLS client configuration that does not authenticate the server.
pub fn insecure_client_config() -> ClientConfig {
    let mut root_store = RootCertStore::empty();
    root_store.add(builtin_cert()).unwrap();
    let mut cfg = ClientConfig::builder().with_root_certificates(root_store).with_no_client_auth();
    cfg.dangerous().set_certificate_verifier(Arc::new(NullVerifier));
    cfg
}

/// TLS client configuration that authenticates the server using the CA certificates from the PEM file.
pub fn client_config_with_ca(ca_path: &Path) -> Result<ClientConfig> {
    let mut root_store = RootCertStore::empty();
    for cert in load_certs(ca_path)? {
        root_store.add(cert).context("invalid CA certificate")?;
    }
    Ok(ClientConfig::builder().with_root_certificates(root_store).with_no_client_auth())
}

/// TLS server configuration using the built-in self-signed certificate.
pub fn builtin_server_config() -> ServerConfig {
    ServerConfig::builder().with_no_client_auth().with_single_cert(vec![builtin_cert()], builtin_key()).unwrap()
}

/// TLS server configuration using the certificate chain and private key from PEM files.
pub fn server_config(cert_path: &Path, key_path: &Path) -> Result<ServerConfig> {
    let certs = load_certs(cert_path)?;
    let mut reader = BufReader::new(File::open(key_path).context("cannot open private key file")?);
    let key = private_key(&mut reader)
        .context("cannot read private key file")?
        .context("no private key found in private key file")?;
    ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .context("invalid certificate or key")
}

/// Loads certificates from a PEM file.
fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let mut reader = BufReader::new(File::open(path).context("cannot open certificate file")?);
    let certs =
        certs(&mut reader).collect::<std::result::Result<Vec<_>, _>>().context("cannot read certificate file")?;
    anyhow::ensure!(!certs.is_empty(), "no certificate found in {}", path.display());
    Ok(certs)
}
