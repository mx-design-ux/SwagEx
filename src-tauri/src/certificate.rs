use anyhow::{Context, bail};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use std::{fs, path::Path};

const CERTIFICATE_FILE: &str = "SwagEx-CA.pem";
const CERTIFICATE_DER_FILE: &str = "SwagEx-CA.cer";
const PRIVATE_KEY_FILE: &str = "SwagEx-CA.key";

pub fn certificate_der_path(directory: &Path) -> std::path::PathBuf {
    directory.join(CERTIFICATE_DER_FILE)
}

#[cfg(target_os = "windows")]
pub fn is_certificate_trusted(directory: &Path) -> anyhow::Result<bool> {
    let der = fs::read(certificate_der_path(directory))?;
    Ok(certificate_is_in_root_store(
        &der,
        windows_sys::Win32::Security::Cryptography::CERT_SYSTEM_STORE_CURRENT_USER,
    )? || certificate_is_in_root_store(
        &der,
        windows_sys::Win32::Security::Cryptography::CERT_SYSTEM_STORE_LOCAL_MACHINE,
    )?)
}

#[cfg(not(target_os = "windows"))]
pub fn is_certificate_trusted(_directory: &Path) -> anyhow::Result<bool> {
    Ok(false)
}

#[cfg(target_os = "windows")]
fn certificate_is_in_root_store(der: &[u8], store_location: u32) -> anyhow::Result<bool> {
    use std::{ffi::c_void, iter, os::windows::ffi::OsStrExt, ptr};
    use windows_sys::Win32::Security::Cryptography::{
        CERT_STORE_OPEN_EXISTING_FLAG, CERT_STORE_PROV_SYSTEM_W, CERT_STORE_READONLY_FLAG,
        CertCloseStore, CertEnumCertificatesInStore, CertFreeCertificateContext, CertOpenStore,
        PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
    };

    let root_store_name = std::ffi::OsStr::new("ROOT")
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let store = unsafe {
        CertOpenStore(
            CERT_STORE_PROV_SYSTEM_W,
            X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
            0,
            store_location | CERT_STORE_OPEN_EXISTING_FLAG | CERT_STORE_READONLY_FLAG,
            root_store_name.as_ptr().cast::<c_void>(),
        )
    };
    if store.is_null() {
        return Err(std::io::Error::last_os_error()).context(
            "Windows n’a pas permis de vérifier le magasin des autorités racines de confiance",
        );
    }

    let mut previous = ptr::null();
    let mut found = false;
    loop {
        let current = unsafe { CertEnumCertificatesInStore(store, previous) };
        if current.is_null() {
            break;
        }
        previous = current;
        let context = unsafe { &*current };
        let encoded = unsafe {
            std::slice::from_raw_parts(context.pbCertEncoded, context.cbCertEncoded as usize)
        };
        if encoded == der {
            found = true;
            unsafe {
                CertFreeCertificateContext(current);
            }
            break;
        }
    }

    let closed = unsafe { CertCloseStore(store, 0) };
    if closed == 0 {
        return Err(std::io::Error::last_os_error())
            .context("Windows n’a pas pu terminer la vérification du certificat");
    }
    Ok(found)
}

#[cfg(unix)]
fn restrict_private_key_permissions(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_private_key_permissions(_path: &Path) -> anyhow::Result<()> {
    // Windows stores application data in the current user's private profile.
    // POSIX mode bits do not exist there; its ACL remains the authority.
    Ok(())
}

/// Builds an iOS configuration profile containing only the public SwagEx CA.
///
/// A manually installed root certificate still requires explicit trust in
/// iOS. The profile is public-only; iOS controls its download and installation
/// flow, including the explicit confirmation screens.
pub fn mobileconfig_profile(der: &[u8]) -> Vec<u8> {
    let encoded_der = STANDARD.encode(der);
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>PayloadContent</key>
    <array>
        <dict>
            <key>PayloadCertificateFileName</key>
            <string>SwagEx.cer</string>
            <key>PayloadContent</key>
            <data>{encoded_der}</data>
            <key>PayloadDisplayName</key>
            <string>SwagEx</string>
            <key>PayloadIdentifier</key>
            <string>com.swagex.local-ca</string>
            <key>PayloadType</key>
            <string>com.apple.security.root</string>
            <key>PayloadUUID</key>
            <string>3C2A4D4E-6AEF-4F8D-8FCE-3F0A4C8CB9A1</string>
            <key>PayloadVersion</key>
            <integer>1</integer>
        </dict>
    </array>
    <key>PayloadDisplayName</key>
    <string>SwagEx</string>
    <key>PayloadIdentifier</key>
    <string>com.swagex.profile</string>
    <key>PayloadOrganization</key>
    <string>SwagEx</string>
    <key>PayloadRemovalDisallowed</key>
    <false/>
    <key>PayloadType</key>
    <string>Configuration</string>
    <key>PayloadUUID</key>
    <string>91D4BEF7-3DA9-4B3B-8E77-2BBA69E9CC26</string>
    <key>PayloadVersion</key>
    <integer>1</integer>
</dict>
</plist>
"#,
    )
    .into_bytes()
}

pub struct CertificateMaterial {
    pub issuer: Issuer<'static, KeyPair>,
    pub der: Vec<u8>,
    pub was_created: bool,
}

pub fn ensure_certificate(directory: &Path) -> anyhow::Result<CertificateMaterial> {
    let directory_existed = directory.exists();
    fs::create_dir_all(directory)?;
    let certificate_path = directory.join(CERTIFICATE_FILE);
    let certificate_der_path = certificate_der_path(directory);
    let private_key_path = directory.join(PRIVATE_KEY_FILE);

    let certificate_exists = certificate_path.exists();
    let certificate_der_exists = certificate_der_path.exists();
    let private_key_exists = private_key_path.exists();
    let all_files_exist = certificate_exists && certificate_der_exists && private_key_exists;
    let no_files_exist = !certificate_exists && !certificate_der_exists && !private_key_exists;

    // A normal first launch creates the directory and all three files. Once a
    // CA exists, silently creating a replacement would invalidate the trust
    // relationship already installed on the Apple device. Require an explicit
    // regeneration instead of rotating the certificate behind the user's back.
    if !all_files_exist && (directory_existed || !no_files_exist) {
        bail!(
            "Le certificat SwagEx est incomplet. Utilisez « Nouveau certificat… » uniquement pour le remplacer volontairement."
        );
    }

    let was_created = !all_files_exist;

    if was_created {
        let mut params = CertificateParams::new(Vec::<String>::new())?;
        let mut distinguished_name = DistinguishedName::new();
        distinguished_name.push(DnType::CommonName, "SwagEx");
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
        restrict_private_key_permissions(&private_key_path)?;
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

pub fn regenerate_certificate(directory: &Path) -> anyhow::Result<CertificateMaterial> {
    if directory.exists() {
        fs::remove_dir_all(directory)?;
    }
    ensure_certificate(directory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_then_reuses_a_private_ca() {
        let directory = tempfile::tempdir().unwrap();
        let certificate_directory = directory.path().join("certificate");
        let first = ensure_certificate(&certificate_directory).unwrap();
        assert!(first.was_created);
        assert!(!first.der.is_empty());

        let second = ensure_certificate(&certificate_directory).unwrap();
        assert!(!second.was_created);
        assert_eq!(first.der, second.der);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mode = fs::metadata(certificate_directory.join(PRIVATE_KEY_FILE))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn refuses_to_rotate_an_existing_incomplete_ca() {
        let directory = tempfile::tempdir().unwrap();
        let certificate_directory = directory.path().join("certificate");
        let first = ensure_certificate(&certificate_directory).unwrap();
        fs::remove_file(certificate_directory.join(PRIVATE_KEY_FILE)).unwrap();

        let error = ensure_certificate(&certificate_directory).err().unwrap();
        assert!(
            error
                .to_string()
                .contains("certificat SwagEx est incomplet")
        );
        assert!(!first.der.is_empty());
    }

    #[test]
    fn builds_a_mobileconfig_with_only_the_public_certificate() {
        let der = [0x30, 0x03, 0x02, 0x01, 0x00];
        let profile = String::from_utf8(mobileconfig_profile(&der)).unwrap();

        assert!(profile.contains("com.apple.security.root"));
        assert!(profile.contains("SwagEx"));
        assert!(!profile.contains("SwagEx Local CA"));
        assert!(profile.contains(&STANDARD.encode(der)));
        assert!(!profile.contains("PRIVATE KEY"));
    }
}
