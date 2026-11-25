use std::sync::Arc;
use std::path::{Path, PathBuf};
use rustls::{ClientConfig, ServerConfig, RootCertStore};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::client::danger::{ServerCertVerified, ServerCertVerifier, HandshakeSignatureValid};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, CryptoProvider, WebPkiSupportedAlgorithms};
use rustls::DistinguishedName;
use rustls::SignatureScheme;
use crate::Result;

/// Self-signed TLS credentials bound to a node identity (CN = node_id)
pub struct TlsCredentials {
    pub node_id: String,
    cert_der: Vec<u8>,
    key_der: Vec<u8>,
}

impl TlsCredentials {
    /// Load persisted credentials or generate+save new self-signed cert.
    /// Cert+key are stored in a single file written atomically (temp + rename)
    /// so concurrent writers/readers never observe a mismatched pair.
    pub fn load_or_generate(dir: &Path, node_id: &str) -> Result<Self> {
        let path = dir.join("tls_credentials.bin");
        if path.exists() {
            let raw = std::fs::read(&path)
                .map_err(|e| crate::NetworkError::ConnectionError(format!("TLS creds read: {}", e)))?;
            if let Some(creds) = Self::decode(&raw) {
                let creds = Self { node_id: node_id.to_string(), ..creds };
                // Verify the persisted pair is actually usable
                if creds.server_config().is_ok() {
                    return Ok(creds);
                }
            }
            // Corrupt/mismatched file — regenerate below
        }
        let creds = Self::generate(node_id)?;
        std::fs::create_dir_all(dir).ok();
        let tmp = dir.join("tls_credentials.tmp");
        if std::fs::write(&tmp, creds.encode()).is_ok() {
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::rename(&tmp, &path);
        }
        Ok(creds)
    }

    /// Serialize cert+key into one blob: [cert_len u32][cert][key_len u32][key]
    fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(self.cert_der.len() as u32).to_be_bytes());
        out.extend_from_slice(&self.cert_der);
        out.extend_from_slice(&(self.key_der.len() as u32).to_be_bytes());
        out.extend_from_slice(&self.key_der);
        out
    }

    fn decode(raw: &[u8]) -> Option<Self> {
        if raw.len() < 8 { return None; }
        let cert_len = u32::from_be_bytes(raw[0..4].try_into().ok()?) as usize;
        if raw.len() < 4 + cert_len + 4 { return None; }
        let cert_der = raw[4..4 + cert_len].to_vec();
        let key_off = 4 + cert_len;
        let key_len = u32::from_be_bytes(raw[key_off..key_off + 4].try_into().ok()?) as usize;
        if raw.len() < key_off + 4 + key_len { return None; }
        let key_der = raw[key_off + 4..key_off + 4 + key_len].to_vec();
        Some(Self { node_id: String::new(), cert_der, key_der })
    }

    /// Generate a self-signed certificate with CN = node_id
    pub fn generate(node_id: &str) -> Result<Self> {
        let key_pair = rcgen::KeyPair::generate()
            .map_err(|e| crate::NetworkError::ConnectionError(format!("TLS keygen: {}", e)))?;
        let mut params = rcgen::CertificateParams::new(vec!["localhost".to_string()])
            .map_err(|e| crate::NetworkError::ConnectionError(format!("TLS params: {}", e)))?;
        params.distinguished_name = rcgen::DistinguishedName::new();
        params.distinguished_name.push(rcgen::DnType::CommonName, node_id);
        // 10-year validity for testnet
        params.not_before = rcgen::date_time_ymd(2025, 1, 1);
        params.not_after = rcgen::date_time_ymd(2035, 1, 1);
        let cert = params.self_signed(&key_pair)
            .map_err(|e| crate::NetworkError::ConnectionError(format!("TLS cert: {}", e)))?;
        Ok(Self {
            node_id: node_id.to_string(),
            cert_der: cert.der().to_vec(),
            key_der: key_pair.serialize_der(),
        })
    }

    /// Build a rustls ServerConfig presenting this certificate
    pub fn server_config(&self) -> Result<ServerConfig> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let cert = CertificateDer::from(self.cert_der.clone());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_der.clone()));
        ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| crate::NetworkError::ConnectionError(format!("TLS protocol versions: {}", e)))?
            .with_client_cert_verifier(Arc::new(XentriumClientCertVerifier::new()))
            .with_single_cert(vec![cert], key)
            .map_err(|e| crate::NetworkError::ConnectionError(format!("TLS server config: {}", e)))
    }

    /// ClientConfig presenting this node's certificate (mutual TLS)
    pub fn client_config(&self) -> Result<ClientConfig> {
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let cert = CertificateDer::from(self.cert_der.clone());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_der.clone()));
        ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| crate::NetworkError::ConnectionError(format!("TLS client protocol: {}", e)))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(XentriumCertVerifier::new()))
            .with_client_auth_cert(vec![cert], key)
            .map_err(|e| crate::NetworkError::ConnectionError(format!("TLS client auth cert: {}", e)))
    }

    /// The certificate DER for distribution to peers (TOFU/pinning)
    pub fn cert_der(&self) -> &[u8] { &self.cert_der }

    /// Days until certificate expiry (negative if expired).
    /// None if the cert cannot be parsed.
    pub fn expiry_days(&self) -> Option<i64> {
        let (_, cert) = x509_parser::parse_x509_certificate(&self.cert_der).ok()?;
        let not_after = cert.validity().not_after.timestamp();
        let now = chrono::Utc::now().timestamp();
        Some((not_after - now) / 86400)
    }

    /// Rotate credentials: generate a fresh pair, verify it parses and
    /// builds usable TLS configs, then atomically replace the file.
    /// Returns the new expiry in days.
    pub fn rotate(dir: &Path, node_id: &str) -> Result<i64> {
        let creds = Self::generate(node_id)?;
        // Replacement verification: cert parses, CN bound, configs build
        let days = creds.expiry_days().ok_or_else(|| {
            crate::NetworkError::ConnectionError("rotated cert failed to parse".into())
        })?;
        creds.server_config()?;
        creds.client_config()?;
        std::fs::create_dir_all(dir).ok();
        let path = dir.join("tls_credentials.bin");
        let tmp = dir.join("tls_credentials.tmp");
        std::fs::write(&tmp, creds.encode())
            .map_err(|e| crate::NetworkError::ConnectionError(format!("TLS write: {}", e)))?;
        let _ = std::fs::remove_file(&path);
        std::fs::rename(&tmp, &path)
            .map_err(|e| crate::NetworkError::ConnectionError(format!("TLS rename: {}", e)))?;
        Ok(days)
    }
}

/// Custom certificate verifier: validates structure + expiry, extracts CN as node identity.
/// Trust model: testnet TOFU — any well-formed, unexpired certificate is accepted at the
/// TLS layer; identity binding is enforced by comparing cert CN to the Ed25519 handshake.
#[derive(Debug)]
pub struct XentriumCertVerifier {
    provider: Arc<CryptoProvider>,
}

impl XentriumCertVerifier {
    pub fn new() -> Self {
        Self { provider: Arc::new(rustls::crypto::ring::default_provider()) }
    }

    /// Extract node_id (CN) from a peer certificate; None if malformed/expired
    pub fn cert_node_id(cert_der: &[u8]) -> Option<String> {
        let (_, cert) = x509_parser::parse_x509_certificate(cert_der).ok()?;
        // Check expiry
        let now = chrono::Utc::now().timestamp();
        if now < cert.validity().not_before.timestamp() || now > cert.validity().not_after.timestamp() {
            return None;
        }
        let cn = cert.subject().iter_common_name().next()?;
        cn.as_str().ok().map(|s| s.to_string())
    }
}

impl ServerCertVerifier for XentriumCertVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        // Reject malformed or expired certificates
        if Self::cert_node_id(end_entity.as_ref()).is_none() {
            return Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::Expired));
        }
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// Server-side verifier for client certificates: requires a well-formed,
/// unexpired certificate with a CN (the peer's node identity).
#[derive(Debug)]
pub struct XentriumClientCertVerifier {
    provider: Arc<CryptoProvider>,
    root_hints: Vec<DistinguishedName>,
}

impl XentriumClientCertVerifier {
    pub fn new() -> Self {
        Self {
            provider: Arc::new(rustls::crypto::ring::default_provider()),
            root_hints: Vec::new(),
        }
    }
}

impl ClientCertVerifier for XentriumClientCertVerifier {
    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &self.root_hints
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> std::result::Result<ClientCertVerified, rustls::Error> {
        if XentriumCertVerifier::cert_node_id(end_entity.as_ref()).is_none() {
            return Err(rustls::Error::InvalidCertificate(
                rustls::CertificateError::Expired));
        }
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// ClientConfig trusting Xentrium self-signed certs (with structural validation)
pub fn client_config() -> Result<ClientConfig> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .map_err(|e| crate::NetworkError::ConnectionError(format!("TLS client protocol: {}", e)))?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(XentriumCertVerifier::new()))
        .with_no_client_auth()
        .pipe(Ok)
}

trait Pipe: Sized { fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T { f(self) } }
impl<T> Pipe for T {}

/// Bundled TLS acceptor + connector for a node
pub struct TlsTransportConfig {
    pub acceptor: tokio_rustls::TlsAcceptor,
    pub connector: tokio_rustls::TlsConnector,
}

impl TlsTransportConfig {
    pub fn new(credentials: &TlsCredentials) -> Result<Self> {
        let server_config = credentials.server_config()?;
        let client_config = credentials.client_config()?;
        Ok(Self {
            acceptor: tokio_rustls::TlsAcceptor::from(Arc::new(server_config)),
            connector: tokio_rustls::TlsConnector::from(Arc::new(client_config)),
        })
    }

    /// Server name used for outbound TLS connections
    pub fn server_name() -> ServerName<'static> {
        ServerName::try_from("localhost").expect("valid server name")
    }
}

/// Path helper
pub fn tls_dir(data_dir: &str) -> PathBuf {
    Path::new(data_dir).join("tls")
}
