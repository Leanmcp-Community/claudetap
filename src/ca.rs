//! CA bootstrap and on-the-fly leaf-certificate minting.
//!
//! On first run we generate a per-user root CA at `~/.claudetap/ca/root.{crt,key}`
//! and reuse it forever. Leaf certs for each upstream host are minted on demand
//! and cached in process memory keyed by SNI hostname.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use parking_lot_compat::Mutex;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, Ia5String, IsCa, KeyPair,
    KeyUsagePurpose, SanType,
};
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use time::{Duration, OffsetDateTime};

use crate::paths;

// rcgen 0.13 returns its `Certificate` and a separately-held `KeyPair`.
// We keep both so we can sign leaves later.
pub struct Ca {
    pub cert: rcgen::Certificate,
    pub key_pair: KeyPair,
    pub cert_pem: String,
    pub key_pem: String,
}

impl Ca {
    /// Load the CA from `~/.claudetap/ca/`, generating it on first run.
    pub fn load_or_generate() -> Result<Self> {
        let cert_path = paths::ca_cert_path()?;
        let key_path = paths::ca_key_path()?;

        if cert_path.exists() && key_path.exists() {
            check_key_perms(&key_path)?;
            let cert_pem = std::fs::read_to_string(&cert_path)
                .with_context(|| format!("reading {}", cert_path.display()))?;
            let key_pem = std::fs::read_to_string(&key_path)
                .with_context(|| format!("reading {}", key_path.display()))?;
            let key_pair =
                KeyPair::from_pem(&key_pem).context("parsing CA private key PEM")?;
            let params = CertificateParams::from_ca_cert_pem(&cert_pem)
                .context("parsing CA certificate PEM")?;
            let cert = params
                .self_signed(&key_pair)
                .context("re-binding loaded CA cert to its key")?;
            Ok(Self {
                cert,
                key_pair,
                cert_pem,
                key_pem,
            })
        } else {
            paths::ensure_dir(&paths::ca_dir()?)?;
            let ca = generate_root()?;
            write_secret(&key_path, ca.key_pem.as_bytes())?;
            std::fs::write(&cert_path, ca.cert_pem.as_bytes())
                .with_context(|| format!("writing {}", cert_path.display()))?;
            Ok(ca)
        }
    }
}

fn generate_root() -> Result<Ca> {
    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "claudetap local root");
    dn.push(DnType::OrganizationName, "claudetap");
    params.distinguished_name = dn;
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
        KeyUsagePurpose::KeyCertSign,
        KeyUsagePurpose::CrlSign,
        KeyUsagePurpose::DigitalSignature,
    ];
    let now = OffsetDateTime::now_utc();
    params.not_before = now - Duration::days(1);
    params.not_after = now + Duration::days(365 * 10);

    let key_pair = KeyPair::generate().context("generating CA key pair")?;
    let cert = params
        .self_signed(&key_pair)
        .context("self-signing CA cert")?;
    let cert_pem = cert.pem();
    let key_pem = key_pair.serialize_pem();
    Ok(Ca {
        cert,
        key_pair,
        cert_pem,
        key_pem,
    })
}

fn write_secret(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::fs::OpenOptions;
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::fs::OpenOptionsExt;
    let mut opts = OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        opts.mode(0o600);
    }
    let mut f = opts
        .open(path)
        .with_context(|| format!("creating secret file {}", path.display()))?;
    f.write_all(bytes)
        .with_context(|| format!("writing secret file {}", path.display()))?;
    Ok(())
}

fn check_key_perms(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let md = std::fs::metadata(path)
            .with_context(|| format!("stat {}", path.display()))?;
        let mode = md.mode() & 0o777;
        if mode & 0o077 != 0 {
            return Err(anyhow!(
                "CA private key {} has insecure mode {:o}; expected 0600",
                path.display(),
                mode
            ));
        }
    }
    let _ = path;
    Ok(())
}

/// In-memory cache of leaf certificates keyed by SNI host.
#[derive(Clone)]
pub struct LeafCertCache {
    ca: Arc<Ca>,
    inner: Arc<Mutex<HashMap<String, Arc<rustls::sign::CertifiedKey>>>>,
}

impl LeafCertCache {
    pub fn new(ca: Arc<Ca>) -> Self {
        Self {
            ca,
            inner: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Return a `CertifiedKey` for `host`, generating + caching it if missing.
    pub fn get_or_mint(&self, host: &str) -> Result<Arc<rustls::sign::CertifiedKey>> {
        if let Some(ck) = self.inner.lock().get(host).cloned() {
            return Ok(ck);
        }
        let ck = mint_leaf(&self.ca, host)?;
        let arc = Arc::new(ck);
        self.inner.lock().insert(host.to_string(), arc.clone());
        Ok(arc)
    }
}

fn mint_leaf(ca: &Ca, host: &str) -> Result<rustls::sign::CertifiedKey> {
    let san = if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        SanType::IpAddress(ip)
    } else {
        SanType::DnsName(Ia5String::try_from(host.to_string()).context("invalid SNI host")?)
    };

    let mut params = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, host);
    params.distinguished_name = dn;
    params.subject_alt_names = vec![san];
    params.is_ca = IsCa::ExplicitNoCa;
    params.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    params.extended_key_usages = vec![
        rcgen::ExtendedKeyUsagePurpose::ServerAuth,
        rcgen::ExtendedKeyUsagePurpose::ClientAuth,
    ];
    let now = OffsetDateTime::now_utc();
    params.not_before = now - Duration::days(1);
    params.not_after = now + Duration::days(825);

    let leaf_key = KeyPair::generate().context("generating leaf key pair")?;
    let leaf_cert = params
        .signed_by(&leaf_key, &ca.cert, &ca.key_pair)
        .context("signing leaf cert with CA")?;

    let leaf_der = CertificateDer::from(leaf_cert.der().to_vec());
    let leaf_key_der = PrivateKeyDer::try_from(leaf_key.serialize_der())
        .map_err(|e| anyhow!("converting leaf key to DER: {e}"))?;

    let signing_key = rustls::crypto::ring::sign::any_supported_type(&leaf_key_der)
        .context("turning leaf key into rustls signer")?;

    // Chain is just the leaf. Clients trust our CA directly via
    // NODE_EXTRA_CA_CERTS / SSL_CERT_FILE / BUN_CA_BUNDLE, so they will build
    // `leaf -> trusted-CA` without us shipping the CA in the chain.
    Ok(rustls::sign::CertifiedKey::new(
        vec![leaf_der],
        signing_key,
    ))
}

// Tiny shim so we can use a non-poisoning Mutex without pulling in parking_lot.
mod parking_lot_compat {
    use std::sync::{Mutex as StdMutex, MutexGuard};

    pub struct Mutex<T> {
        inner: StdMutex<T>,
    }

    impl<T> Mutex<T> {
        pub fn new(t: T) -> Self {
            Self {
                inner: StdMutex::new(t),
            }
        }
        pub fn lock(&self) -> MutexGuard<'_, T> {
            // Poisoning is irrelevant for this in-memory cache; recover transparently.
            match self.inner.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            }
        }
    }
}
