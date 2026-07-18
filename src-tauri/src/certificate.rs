use anyhow::Context;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

const CERTIFICATE_FILE: &str = "SwagEx-CA.pem";
const CERTIFICATE_DER_FILE: &str = "SwagEx-CA.cer";
const PRIVATE_KEY_FILE: &str = "SwagEx-CA.key";

pub struct CertificateMaterial {
    pub issuer: Issuer<'static, KeyPair>,
    pub der: Vec<u8>,
    pub was_created: bool,
}

pub fn ensure_certificate(directory: &Path) -> anyhow::Result<CertificateMaterial> {
    fs::create_dir_all(directory)?;
    let certificate_path = directory.join(CERTIFICATE_FILE);
    let certificate_der_path = directory.join(CERTIFICATE_DER_FILE);
    let private_key_path = directory.join(PRIVATE_KEY_FILE);

    let was_created =
        !(certificate_path.exists() && certificate_der_path.exists() && private_key_path.exists());

    if was_created {
        let mut params = CertificateParams::new(Vec::<String>::new())?;
        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::CommonName, "SwagEx Local CA");
        distinguished_name.push(DnType::OrganizationName, "SwagEx");
        params.distinguished_name = distinguished_name;
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![
            KeyUsagePurpose::DigitalSignature,
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
        ];

        let key_pair = KeyPair::generate()?;
        let certificate = params.self_signed(&key_pair)?;
        fs::write(&certificate_path, certificate.pem())?;
        fs::write(&certificate_der_path, certificate.der().as_ref())?;
        fs::write(&private_key_path, key_pair.serialize_pem())?;
        fs::set_permissions(&private_key_path, fs::Permissions::from_mode(0o600))?;
    }

    let certificate_pem = fs::read_to_string(&certificate_path)
        .with_context(|| format!("lecture de {}", certificate_path.display()))?;
    let private_key_pem = fs::read_to_string(&private_key_path)
        .with_context(|| format!("lecture de {}", private_key_path.display()))?;
    let der = fs::read(&certificate_der_path)
        .with_context(|| format!("lecture de {}", certificate_der_path.display()))?;
    let key_pair = KeyPair::from_pem(&private_key_pem)?;
    let issuer = Issuer::from_ca_cert_pem(&certificate_pem, key_pair)?;

    Ok(CertificateMaterial {
        issuer,
        der,
        was_created,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_then_reuses_a_private_ca() {
        let directory = tempfile::tempdir().unwrap();
        let first = ensure_certificate(directory.path()).unwrap();
        assert!(first.was_created);
        assert!(!first.der.is_empty());

        let second = ensure_certificate(directory.path()).unwrap();
        assert!(!second.was_created);
        assert_eq!(first.der, second.der);

        let mode = fs::metadata(directory.path().join(PRIVATE_KEY_FILE))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
