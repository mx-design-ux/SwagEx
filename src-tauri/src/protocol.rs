use aes::Aes128;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use cbc::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};
use flate2::read::ZlibDecoder;
use serde_json::Value;
use std::io::Read;

type Aes128CbcDecryptor = cbc::Decryptor<Aes128>;

const KEY_MASK: u8 = 0xA7;
const MASKED_PROTOCOL_KEY: [u8; 16] = [
    224, 213, 147, 244, 149, 194, 206, 233, 203, 144, 221, 214, 146, 234, 213, 242,
];

fn protocol_key() -> [u8; 16] {
    MASKED_PROTOCOL_KEY.map(|byte| byte ^ KEY_MASK)
}

pub fn decode_profile(encoded_body: &[u8]) -> anyhow::Result<Value> {
    let encoded = std::str::from_utf8(encoded_body)?.trim();
    let mut encrypted = STANDARD.decode(encoded)?;
    let key = protocol_key();
    let iv = [0_u8; 16];

    let decrypted = Aes128CbcDecryptor::new(&key.into(), &iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut encrypted)
        .map_err(|_| anyhow::anyhow!("impossible de déchiffrer la réponse du profil"))?;

    let mut inflater = ZlibDecoder::new(decrypted);
    let mut json_bytes = Vec::new();
    inflater.read_to_end(&mut json_bytes)?;

    Ok(serde_json::from_slice(&json_bytes)?)
}

pub fn validate_profile(profile: &Value) -> anyhow::Result<()> {
    let root = profile
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("la réponse du profil n'est pas un objet JSON"))?;

    let command = root
        .get("command")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("commande de profil absente"))?;

    if !matches!(command, "HubUserLogin" | "GuestLogin") {
        anyhow::bail!("la réponse reçue n'est pas un profil de connexion");
    }

    if !root.get("building_list").is_some_and(Value::is_array) {
        anyhow::bail!("profil incomplet : building_list est absent");
    }

    if !root.get("unit_list").is_some_and(Value::is_array) {
        anyhow::bail!("profil incomplet : unit_list est absent");
    }

    if !root.contains_key("runes") {
        anyhow::bail!("profil incomplet : runes est absent");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cbc::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
    use flate2::{Compression, write::ZlibEncoder};
    use std::io::Write;

    type Aes128CbcEncryptor = cbc::Encryptor<Aes128>;

    #[test]
    fn decodes_a_profile_response() {
        let expected = serde_json::json!({
            "command": "HubUserLogin",
            "wizard_info": { "wizard_id": 42, "wizard_name": "Test" },
            "building_list": [],
            "unit_list": [],
            "runes": []
        });
        let serialized = serde_json::to_vec(&expected).unwrap();
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(&serialized).unwrap();
        let compressed = encoder.finish().unwrap();

        let key = protocol_key();
        let iv = [0_u8; 16];
        let mut buffer = vec![0_u8; compressed.len() + 16];
        buffer[..compressed.len()].copy_from_slice(&compressed);
        let encrypted = Aes128CbcEncryptor::new(&key.into(), &iv.into())
            .encrypt_padded_mut::<Pkcs7>(&mut buffer, compressed.len())
            .unwrap();
        let encoded = STANDARD.encode(encrypted);

        let decoded = decode_profile(encoded.as_bytes()).unwrap();
        assert_eq!(decoded, expected);
        validate_profile(&decoded).unwrap();
    }

    #[test]
    fn refuses_an_unrelated_response() {
        let value = serde_json::json!({
            "command": "BattleStart",
            "building_list": [],
            "unit_list": [],
            "runes": []
        });
        assert!(validate_profile(&value).is_err());
    }
}
