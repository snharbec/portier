//! Encryption of mail account passwords at rest.

use anyhow::{Result, anyhow};
use base64::{Engine, engine::general_purpose::STANDARD as B64};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit},
};

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf).expect("system random source unavailable");
    buf
}

pub fn random_token() -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes::<32>())
}

/// Returns base64(nonce || ciphertext).
pub fn encrypt(key: &[u8; 32], plaintext: &str) -> Result<String> {
    let cipher = XChaCha20Poly1305::new(key.into());
    let nonce = random_bytes::<24>();
    let ciphertext = cipher
        .encrypt(&XNonce::from(nonce), plaintext.as_bytes())
        .map_err(|_| anyhow!("encryption failed"))?;
    let mut out = nonce.to_vec();
    out.extend(ciphertext);
    Ok(B64.encode(out))
}

pub fn decrypt(key: &[u8; 32], encoded: &str) -> Result<String> {
    let data = B64.decode(encoded)?;
    if data.len() < 24 {
        return Err(anyhow!("ciphertext too short"));
    }
    let (nonce, ciphertext) = data.split_at(24);
    let cipher = XChaCha20Poly1305::new(key.into());
    let nonce = XNonce::try_from(nonce).map_err(|_| anyhow!("bad nonce"))?;
    let plaintext = cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|_| anyhow!("decryption failed (wrong master key?)"))?;
    Ok(String::from_utf8(plaintext)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let key = random_bytes::<32>();
        let enc = encrypt(&key, "s3cret pässword").unwrap();
        assert_ne!(enc, "s3cret pässword");
        assert_eq!(decrypt(&key, &enc).unwrap(), "s3cret pässword");
    }

    #[test]
    fn wrong_key_fails() {
        let enc = encrypt(&random_bytes::<32>(), "x").unwrap();
        assert!(decrypt(&random_bytes::<32>(), &enc).is_err());
    }
}
