//! What DPAPI does on Windows: data only this user on this Mac can open again. A random key
//! is kept in the login keychain (see [`super::keychain`]); the data is sealed with it.

use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use zeroize::Zeroizing;

/// Not under `Stacker:`, so the vault's mirror never takes it for one of its own.
#[cfg(not(test))]
const SERVICE: &str = "Stacker data protection key";
const MAGIC: &[u8] = b"SKP1";
const NONCE: usize = 24;

#[cfg(not(test))]
static KEY: std::sync::Mutex<Option<Zeroizing<[u8; 32]>>> = std::sync::Mutex::new(None);

/// Tests never touch the user's keychain.
#[cfg(test)]
fn key(_: bool) -> Result<Zeroizing<[u8; 32]>, String> {
    Ok(Zeroizing::new([9u8; 32]))
}

#[cfg(not(test))]
fn key(create: bool) -> Result<Zeroizing<[u8; 32]>, String> {
    let mut held = KEY.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(key) = held.as_ref() {
        return Ok(key.clone());
    }
    use super::keychain::Lookup;
    let key = match super::keychain::read(SERVICE) {
        Lookup::Found(bytes) if bytes.len() == 32 => {
            let mut key = Zeroizing::new([0u8; 32]);
            key.copy_from_slice(&bytes);
            key
        }
        Lookup::Found(_) => return Err("钥匙串里的数据保护密钥已损坏".into()),
        // Only a key that is not there is made: a locked keychain or a refused read must
        // never lead to a new key in place of the one existing data was sealed with.
        Lookup::Missing if create => {
            let mut key = Zeroizing::new([0u8; 32]);
            getrandom::getrandom(key.as_mut()).map_err(|e| e.to_string())?;
            super::keychain::create(SERVICE, key.as_ref())
                .map_err(|_| "钥匙串保存了数据保护密钥，但读不回来".to_string())?;
            key
        }
        Lookup::Missing => return Err("钥匙串里没有数据保护密钥".into()),
        Lookup::Failed => {
            return Err("暂时读不到钥匙串（可能没有解锁），请解锁登录钥匙串后重试".into())
        }
    };
    *held = Some(key.clone());
    Ok(key)
}

pub fn seal(plain: &[u8]) -> Result<Vec<u8>, String> {
    let key = key(true)?;
    seal_with(&key, plain)
}

pub fn open(sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
    let key = key(false)?;
    open_with(&key, sealed)
}

fn seal_with(key: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>, String> {
    let mut nonce = [0u8; NONCE];
    getrandom::getrandom(&mut nonce).map_err(|e| e.to_string())?;
    let cipher = XChaCha20Poly1305::new(key.into());
    let body = cipher
        .encrypt(XNonce::from_slice(&nonce), plain)
        .map_err(|_| "加密失败".to_string())?;
    let mut out = Vec::with_capacity(MAGIC.len() + NONCE + body.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&body);
    Ok(out)
}

fn open_with(key: &[u8; 32], sealed: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
    let rest = sealed
        .strip_prefix(MAGIC)
        .filter(|rest| rest.len() > NONCE)
        .ok_or("不是本机加密的数据")?;
    let (nonce, body) = rest.split_at(NONCE);
    XChaCha20Poly1305::new(key.into())
        .decrypt(XNonce::from_slice(nonce), body)
        .map(Zeroizing::new)
        .map_err(|_| "解密失败：数据已损坏，或不是这台 Mac 上的这个用户加密的".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_data_opens_with_its_key_only() {
        let key = [7u8; 32];
        let sealed = seal_with(&key, b"secret").unwrap();
        assert!(!sealed.windows(6).any(|w| w == b"secret"));
        assert_eq!(open_with(&key, &sealed).unwrap().as_slice(), b"secret");
        assert!(open_with(&[8u8; 32], &sealed).is_err());
        assert!(open_with(&key, b"secret").is_err());
    }
}
