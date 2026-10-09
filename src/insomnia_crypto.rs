//! Insomnia's encrypted, gzip-compressed, content-addressed cloud blobs.

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use openssl::{
    bn::BigNum,
    encrypt::Decrypter,
    hash::MessageDigest,
    pkey::PKey,
    rsa::{Padding, Rsa},
    symm::{self, Cipher},
};
use serde_json::{json, Value};
use std::io::{Read, Write};

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn unhex(text: &str) -> Result<Vec<u8>, String> {
    if !text.len().is_multiple_of(2) || !text.is_ascii() {
        return Err("Invalid encrypted cloud data".into());
    }
    text.as_bytes()
        .chunks(2)
        .map(|pair| {
            let digit = |byte: u8| {
                (byte as char)
                    .to_digit(16)
                    .ok_or_else(|| "Invalid encrypted cloud data".to_string())
            };
            Ok((digit(pair[0])? * 16 + digit(pair[1])?) as u8)
        })
        .collect()
}

pub fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value[key]
        .as_str()
        .ok_or_else(|| format!("Insomnia data is missing {key}"))
}

pub(crate) fn decode_uri_json(bytes: &[u8]) -> Result<Value, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "Invalid encrypted key text".to_string())?;
    // encodeURIComponent leaves '+' literal, unlike form encoding.
    serde_json::from_str(&crate::kv::url_decode(&text.replace('+', "%2B")))
        .map_err(|_| "Invalid encrypted key JSON".to_string())
}

pub fn aes_key(jwk: &Value) -> Result<Vec<u8>, String> {
    let key = URL_SAFE_NO_PAD
        .decode(string(jwk, "k")?)
        .map_err(|_| "Invalid Insomnia AES key".to_string())?;
    if key.len() != 32 {
        return Err("Insomnia AES key must contain 256 bits".into());
    }
    Ok(key)
}

pub fn decrypt(key: &[u8], encrypted: &Value) -> Result<Vec<u8>, String> {
    let iv = unhex(string(encrypted, "iv")?)?;
    let tag = unhex(string(encrypted, "t")?)?;
    if key.len() != 32 || iv.len() != 12 || tag.len() != 16 {
        return Err("Invalid Insomnia AES-GCM parameters".into());
    }
    symm::decrypt_aead(
        Cipher::aes_256_gcm(),
        key,
        Some(&iv),
        &unhex(string(encrypted, "ad")?)?,
        &unhex(string(encrypted, "d")?)?,
        &tag,
    )
    .map_err(|_| "Cloud decryption failed authentication".to_string())
}

pub fn encrypt(key: &[u8], content: &[u8]) -> Result<Value, String> {
    if key.len() != 32 {
        return Err("Invalid Insomnia AES key".into());
    }
    let mut iv = [0; 12];
    openssl::rand::rand_bytes(&mut iv)
        .map_err(|_| "Cannot generate encryption nonce".to_string())?;
    let mut tag = [0; 16];
    let data = symm::encrypt_aead(
        Cipher::aes_256_gcm(),
        key,
        Some(&iv),
        &[],
        content,
        &mut tag,
    )
    .map_err(|_| "Cloud encryption failed".to_string())?;
    Ok(json!({"iv": hex(&iv), "t": hex(&tag), "ad": "", "d": hex(&data)}))
}

pub fn project_key(session: &Value, encrypted_project_key: &str) -> Result<Vec<u8>, String> {
    let private = decode_uri_json(&decrypt(
        &aes_key(&session["symmetricKey"])?,
        &session["encPrivateKey"],
    )?)?;
    let component = |name: &str| -> Result<BigNum, String> {
        let bytes = URL_SAFE_NO_PAD
            .decode(string(&private, name)?)
            .map_err(|_| "Invalid RSA key".to_string())?;
        BigNum::from_slice(&bytes).map_err(|_| "Invalid RSA key".to_string())
    };
    let rsa = Rsa::from_private_components(
        component("n")?,
        component("e")?,
        component("d")?,
        component("p")?,
        component("q")?,
        component("dp")?,
        component("dq")?,
        component("qi")?,
    )
    .map_err(|_| "Invalid Insomnia RSA key".to_string())?;
    let pkey = PKey::from_rsa(rsa).map_err(|_| "Invalid Insomnia RSA key".to_string())?;
    let mut decrypter =
        Decrypter::new(&pkey).map_err(|_| "Cannot initialize RSA decryption".to_string())?;
    decrypter
        .set_rsa_padding(Padding::PKCS1_OAEP)
        .map_err(|_| "Cannot configure RSA padding".to_string())?;
    decrypter
        .set_rsa_oaep_md(MessageDigest::sha256())
        .map_err(|_| "Cannot configure RSA OAEP".to_string())?;
    decrypter
        .set_rsa_mgf1_md(MessageDigest::sha256())
        .map_err(|_| "Cannot configure RSA MGF1".to_string())?;
    let encrypted = unhex(encrypted_project_key)?;
    let mut output = vec![
        0;
        decrypter
            .decrypt_len(&encrypted)
            .map_err(|_| "Invalid encrypted project key".to_string())?
    ];
    let len = decrypter
        .decrypt(&encrypted, &mut output)
        .map_err(|_| "Project key decryption failed".to_string())?;
    aes_key(&decode_uri_json(&output[..len])?)
}

pub fn encode_document(document: &Value) -> Result<(String, Vec<u8>), String> {
    let mut document = document.clone();
    let object = document
        .as_object_mut()
        .ok_or_else(|| "Invalid cloud resource".to_string())?;
    object.remove("modified");
    if matches!(
        object.get("type").and_then(Value::as_str),
        Some("Workspace" | "ProjectLintRuleset")
    ) {
        object.insert("parentId".into(), Value::Null);
    }
    // serde_json's default map is sorted, matching Insomnia's deterministic stringify.
    let content =
        serde_json::to_vec(&document).map_err(|_| "Cannot encode cloud resource".to_string())?;
    let id = hex(&openssl::sha::sha1(&content));
    let mut gzip = GzEncoder::new(Vec::new(), Compression::default());
    gzip.write_all(&content)
        .map_err(|error| format!("Cannot compress cloud resource: {error}"))?;
    let compressed = gzip
        .finish()
        .map_err(|error| format!("Cannot compress cloud resource: {error}"))?;
    Ok((id, compressed))
}

pub fn decode_document(key: &[u8], id: &str, encrypted: &Value) -> Result<Value, String> {
    let compressed = decrypt(key, encrypted)?;
    let mut content = Vec::new();
    GzDecoder::new(compressed.as_slice())
        .take(32 * 1024 * 1024 + 1)
        .read_to_end(&mut content)
        .map_err(|_| "Invalid compressed cloud resource".to_string())?;
    if content.len() > 32 * 1024 * 1024 || hex(&openssl::sha::sha1(&content)) != id {
        return Err("Cloud resource does not match its content ID".into());
    }
    serde_json::from_slice(&content).map_err(|_| "Invalid cloud resource JSON".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes256_gcm_matches_known_vector_and_rejects_tampering() {
        let encrypted = json!({"iv": "000000000000000000000000", "ad": "",
            "t": "d0d1c8a799996bf0265b98b5d48ab919", "d": "cea7403d4d606b6e074ec5d3baf39d18"});
        assert_eq!(decrypt(&[0; 32], &encrypted).unwrap(), vec![0; 16]);
        let mut bad = encrypted.clone();
        bad["t"] = json!("00".repeat(16));
        assert!(decrypt(&[0; 32], &bad).is_err());
        assert!(unhex("é").is_err());
    }

    #[test]
    fn blobs_roundtrip_and_verify_hashes() {
        let doc =
            json!({"_id": "req_example", "type": "Request", "url": "{{ _.host }}", "modified": 42});
        let (id, compressed) = encode_document(&doc).unwrap();
        let encrypted = encrypt(&[1; 32], &compressed).unwrap();
        let decoded = decode_document(&[1; 32], &id, &encrypted).unwrap();
        assert_eq!(decoded["url"], doc["url"]);
        assert!(decoded.get("modified").is_none());
        assert!(decode_document(&[1; 32], "wrong", &encrypted).is_err());
        assert!(decode_document(&[2; 32], &id, &encrypted).is_err());
    }
}
