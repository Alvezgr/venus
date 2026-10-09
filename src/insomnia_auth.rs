//! Browser sign-in uses Insomnia's sealed authentication code, never a password.

use crate::{insomnia_crypto as crypto, insomnia_sync::Cloud, insomnia_workspace, storage};
use base64::{engine::general_purpose::STANDARD, Engine};
use crypto_box::SecretKey;
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub struct LoginChallenge {
    secret: SecretKey,
    started: Instant,
}

impl LoginChallenge {
    pub fn new() -> Result<Self, String> {
        let mut bytes = [0; 32];
        openssl::rand::rand_bytes(&mut bytes)
            .map_err(|_| "Cannot create sign-in key".to_string())?;
        Ok(Self {
            secret: SecretKey::from(bytes),
            started: Instant::now(),
        })
    }

    pub fn url(&self) -> String {
        let key = STANDARD.encode(self.secret.public_key().as_bytes());
        // Insomnia applies encodeURIComponent before URLSearchParams encoding.
        let encoded = crate::kv::url_encode(&key);
        let mut url = reqwest::Url::parse("https://app.insomnia.rest/app/auth-app/").unwrap();
        url.query_pairs_mut()
            .append_pair("loginKey", &encoded)
            .append_pair("source_origin", "desktop_app");
        url.into()
    }

    fn decode(&self, code: &str) -> Result<Value, String> {
        if self.started.elapsed() > Duration::from_secs(15 * 60) {
            return Err("Sign-in expired; start browser sign-in again".into());
        }
        if code.len() > 64 * 1024 {
            return Err("Invalid authentication code".into());
        }
        let bytes = STANDARD
            .decode(code.trim())
            .map_err(|_| "Invalid authentication code".to_string())?;
        let plaintext = self.secret.unseal(&bytes).map_err(|_| {
            "Invalid authentication code; use the code from this sign-in window".to_string()
        })?;
        serde_json::from_slice(&plaintext).map_err(|_| "Invalid authentication code".to_string())
    }

    pub fn complete(&self, code: &str) -> Result<Value, String> {
        let authorization = self.decode(code)?;
        let token = crypto::string(&authorization, "token")?;
        let cloud = Cloud::from_session("", json!({"id": token}))?;
        let account = cloud.get("/v3/users/me")?;
        let keys = cloud.get("/v3/users/me/encryption-keys")?;
        hydrate_session(token, &authorization["key"], &account, &keys)
    }
}

fn parse_key(value: &Value, field: &str) -> Result<Value, String> {
    serde_json::from_str(crypto::string(value, field)?)
        .map_err(|_| "Invalid account encryption keys".to_string())
}

fn hydrate_session(
    token: &str,
    login_key: &Value,
    account: &Value,
    keys: &Value,
) -> Result<Value, String> {
    let symmetric_key = crypto::decode_uri_json(&crypto::decrypt(
        &crypto::aes_key(login_key)?,
        &parse_key(keys, "enc_symmetric_key")?,
    )?)?;
    crypto::aes_key(&symmetric_key)?;
    let private = parse_key(keys, "enc_private_key")?;
    crypto::decode_uri_json(&crypto::decrypt(
        &crypto::aes_key(&symmetric_key)?,
        &private,
    )?)?;
    Ok(
        json!({"id": token, "accountId": crypto::string(account, "id")?,
        "email": crypto::string(account, "email")?, "firstName": account["first_name"],
        "lastName": account["last_name"], "symmetricKey": symmetric_key,
        "publicKey": parse_key(keys, "public_key")?, "encPrivateKey": private}),
    )
}

fn session_path() -> PathBuf {
    storage::dir().join("insomnia/session.json")
}

pub fn native_session() -> Result<Value, String> {
    let directory = dirs::config_dir()
        .ok_or_else(|| "Cannot locate Insomnia's login".to_string())?
        .join("Insomnia");
    let content = fs::read_to_string(directory.join("insomnia.UserSession.db"))
        .map_err(|_| "No installed Insomnia login found; sign in with your browser".to_string())?;
    crate::insomnia_sync::read_session(&content)
}

pub fn load_session() -> Result<Value, String> {
    let path = session_path();
    if !path.exists() {
        return native_session();
    }
    let session: Value = serde_json::from_slice(
        &fs::read(path).map_err(|_| "Cannot read Mercury's Insomnia login".to_string())?,
    )
    .map_err(|_| "Invalid saved login; sign in again".to_string())?;
    if session["id"].as_str().is_none_or(str::is_empty) {
        return Err("Sign in to Insomnia Cloud".into());
    }
    Ok(session)
}

pub fn save_session(session: &Value) -> Result<(), String> {
    let path = session_path();
    let parent = path.parent().unwrap();
    fs::create_dir_all(parent).map_err(|_| "Cannot create login directory".to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
            .map_err(|_| "Cannot protect login directory".to_string())?;
    }
    insomnia_workspace::atomic_write(
        &path,
        &serde_json::to_vec(session).map_err(|_| "Cannot save login".to_string())?,
    )
}

pub fn sign_out() -> Result<(), String> {
    // A signed-out marker prevents silently reusing the installed app's session.
    save_session(&Value::Null)
}

pub fn account_collections(
    session: Value,
) -> Result<(String, Vec<crate::insomnia_sync::Collection>), String> {
    let cloud = Cloud::from_session("", session)?;
    let account = cloud.get("/v3/users/me")?;
    let email = crypto::string(&account, "email")?.to_string();
    Ok((email, cloud.collections()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crypto_box::aead::OsRng;

    #[test]
    fn authentication_code_is_bound_to_the_login_window() {
        let challenge = LoginChallenge::new().unwrap();
        let code = challenge
            .secret
            .public_key()
            .seal(&mut OsRng, br#"{"token":"example","key":{}}"#)
            .unwrap();
        assert_eq!(
            challenge.decode(&STANDARD.encode(&code)).unwrap()["token"],
            "example"
        );
        assert!(LoginChallenge::new()
            .unwrap()
            .decode(&STANDARD.encode(&code))
            .is_err());
        let mut tampered = code;
        *tampered.last_mut().unwrap() ^= 1;
        assert!(challenge.decode(&STANDARD.encode(tampered)).is_err());
        assert!(challenge.decode("not a code").is_err());
    }

    #[test]
    fn login_url_contains_only_the_public_key() {
        let challenge = LoginChallenge::new().unwrap();
        let url = reqwest::Url::parse(&challenge.url()).unwrap();
        let expected =
            crate::kv::url_encode(&STANDARD.encode(challenge.secret.public_key().as_bytes()));
        assert_eq!(
            url.query_pairs()
                .find(|(key, _)| key == "loginKey")
                .unwrap()
                .1,
            expected
        );
        assert!(!url
            .as_str()
            .contains(&STANDARD.encode(challenge.secret.to_bytes())));
    }

    #[test]
    fn opens_a_native_libsodium_authentication_code_and_checks_expiry() {
        // Generated by libsodium crypto_box_seal with a synthetic payload.
        let mut challenge = LoginChallenge {
            secret: SecretKey::from(std::array::from_fn(|i| i as u8)),
            started: Instant::now(),
        };
        let code = "sKXTxbctVsRztMYYEffyNulZsFjBeqptrFfHw05txHbqXEch/ry9eDonBqmRnlE9aZg2lW0mTGF8VwDaqCDascq9QEDWX/+cXQR7yQ==";
        assert_eq!(challenge.decode(code).unwrap()["token"], "example");
        challenge.started -= Duration::from_secs(16 * 60);
        assert!(challenge.decode(code).is_err());
    }

    #[test]
    fn account_keys_are_decrypted_and_tampering_is_rejected() {
        let login_key = json!({"k":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"});
        let account_key = json!({"k":"AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE"});
        let encrypted = crypto::encrypt(
            &crypto::aes_key(&login_key).unwrap(),
            account_key.to_string().as_bytes(),
        )
        .unwrap();
        let private =
            crypto::encrypt(&crypto::aes_key(&account_key).unwrap(), br#"{"kty":"RSA"}"#).unwrap();
        let keys = json!({"enc_symmetric_key":encrypted.to_string(), "enc_private_key":private.to_string(), "public_key":"{}"});
        let account = json!({"id":"acct_example", "email":"example@example.com"});
        let session = hydrate_session("example", &login_key, &account, &keys).unwrap();
        assert_eq!(session["symmetricKey"], account_key);
        assert_eq!(session["accountId"], "acct_example");
        let wrong_key = json!({"k":"AgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgI"});
        assert!(hydrate_session("example", &wrong_key, &account, &keys).is_err());
    }
}
