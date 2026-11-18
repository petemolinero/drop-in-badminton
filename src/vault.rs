//! Encrypting the data file when a password is set.
//!
//! The password is stretched with Argon2id into a 256-bit key, and the whole
//! database is sealed with XChaCha20-Poly1305, which also detects a wrong
//! password or a damaged file. Nothing about the password is stored: only the
//! salt and Argon2 settings, in the clear at the start of the file.
//!
//! File layout: `DIBVAULT`, version (1 byte), Argon2 memory KiB, passes and
//! lanes (3 × u32 little-endian), salt (16), nonce (24), then the sealed
//! database. Everything before the sealed part is authenticated with it.

use std::io::Write;
use std::path::Path;

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

const MAGIC: &[u8; 8] = b"DIBVAULT";
const FORMAT: u8 = 1;
const HEADER: usize = 8 + 1 + 12 + 16 + 24;

/// Argon2id settings: OWASP's recommended minimum (19 MiB, 2 passes), about
/// a fifth of a second on an older laptop. Stored per file, so they can be
/// raised later without breaking old files.
const MEMORY_KIB: u32 = 19 * 1024;
const PASSES: u32 = 2;
const LANES: u32 = 1;

#[derive(Debug, PartialEq, Eq)]
pub enum VaultError {
    /// Not one of our files (or cut short).
    NotAVault,
    /// The password doesn't open it (or the file was tampered with).
    WrongPassword,
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            VaultError::NotAVault => "the data file isn't readable",
            VaultError::WrongPassword => "that's not the password",
        })
    }
}

/// A key derived from the password, with what's needed to derive it again.
#[derive(Clone)]
pub struct Key {
    key: [u8; 32],
    salt: [u8; 16],
    params: [u32; 3],
}

impl Key {
    /// A fresh key for a new password (new random salt).
    pub fn new(password: &str) -> Key {
        let mut salt = [0u8; 16];
        getrandom::fill(&mut salt).expect("the system's random number generator");
        Self::derive(password, salt, [MEMORY_KIB, PASSES, LANES]).expect("valid Argon2 settings")
    }

    fn derive(password: &str, salt: [u8; 16], params: [u32; 3]) -> Option<Key> {
        let [m, t, p] = params;
        let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, Params::new(m, t, p, Some(32)).ok()?);
        let mut key = [0u8; 32];
        argon.hash_password_into(password.as_bytes(), &salt, &mut key).ok()?;
        Some(Key { key, salt, params })
    }

    /// Whether `password` is the one this key came from.
    pub fn matches(&self, password: &str) -> bool {
        Self::derive(password, self.salt, self.params).is_some_and(|k| k.key == self.key)
    }
}

/// Seal `plain` (a whole database) with `key`.
pub fn seal(key: &Key, plain: &[u8]) -> Vec<u8> {
    let mut nonce = [0u8; 24];
    getrandom::fill(&mut nonce).expect("the system's random number generator");
    let mut out = Vec::with_capacity(HEADER + plain.len() + 16);
    out.extend_from_slice(MAGIC);
    out.push(FORMAT);
    for v in key.params {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&key.salt);
    out.extend_from_slice(&nonce);
    let cipher = XChaCha20Poly1305::new(&key.key.into());
    let sealed = cipher.encrypt(XNonce::from_slice(&nonce), Payload { msg: plain, aad: &out }).expect("encryption");
    out.extend_from_slice(&sealed);
    out
}

fn header(file: &[u8]) -> Result<([u8; 16], [u32; 3]), VaultError> {
    if file.len() < HEADER + 16 || &file[..8] != MAGIC || file[8] != FORMAT {
        return Err(VaultError::NotAVault);
    }
    let u32_at = |i: usize| u32::from_le_bytes(file[i..i + 4].try_into().expect("4 bytes"));
    let params = [u32_at(9), u32_at(13), u32_at(17)];
    // Refuse absurd settings rather than try to allocate them.
    if params[0] > 1024 * 1024 || params[1] > 64 || params[2] > 16 {
        return Err(VaultError::NotAVault);
    }
    Ok((file[21..37].try_into().expect("16 bytes"), params))
}

/// Open a sealed file with a key already derived (same salt).
pub fn open_with(key: &Key, file: &[u8]) -> Result<Vec<u8>, VaultError> {
    let (salt, params) = header(file)?;
    if salt != key.salt || params != key.params {
        return Err(VaultError::WrongPassword);
    }
    let cipher = XChaCha20Poly1305::new(&key.key.into());
    let nonce = XNonce::from_slice(&file[37..HEADER]);
    cipher.decrypt(nonce, Payload { msg: &file[HEADER..], aad: &file[..HEADER] }).map_err(|_| VaultError::WrongPassword)
}

/// Open a sealed file with the password: its key and contents.
pub fn open(password: &str, file: &[u8]) -> Result<(Key, Vec<u8>), VaultError> {
    let (salt, params) = header(file)?;
    let key = Key::derive(password, salt, params).ok_or(VaultError::NotAVault)?;
    let plain = open_with(&key, file)?;
    Ok((key, plain))
}

/// Write a file so that a crash leaves either the old or the new contents,
/// never half of each.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seals_and_opens_only_with_the_password() {
        let key = Key::new("correct horse");
        let file = seal(&key, b"the whole database");
        assert!(!file.windows(5).any(|w| w == b"whole"), "contents are not readable");
        let (again, plain) = open("correct horse", &file).unwrap();
        assert_eq!(plain, b"the whole database");
        assert!(again.matches("correct horse") && !again.matches("Correct horse"));
        assert_eq!(open("wrong", &file).err(), Some(VaultError::WrongPassword));
        assert_eq!(open_with(&key, &file).unwrap(), b"the whole database");

        // Any change to the file is caught.
        let mut tampered = file.clone();
        *tampered.last_mut().unwrap() ^= 1;
        assert_eq!(open("correct horse", &tampered).err(), Some(VaultError::WrongPassword));
        assert_eq!(open("x", b"SQLite format 3\0").err(), Some(VaultError::NotAVault));
    }
}
