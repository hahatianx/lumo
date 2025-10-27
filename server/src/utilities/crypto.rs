use crate::err::Result;
use crate::global_var::ENV_VAR;
use aes::Aes256;
use bytes::Bytes;
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use cbc::{Decryptor, Encryptor};
use sha2::{Digest, Sha256};
use std::sync::LazyLock;

type Aes256Cbc = Encryptor<Aes256>;
type Aes256CbcDec = Decryptor<Aes256>;

static KEY: LazyLock<[u8; 32]> = LazyLock::new(|| get_key());

fn get_key() -> [u8; 32] {
    let seed = ENV_VAR.get().unwrap().get_conn_token();
    let mut hasher = Sha256::new();
    hasher.update(seed.as_bytes());
    hasher.finalize().into()
}

#[inline]
pub fn encrypt(data: Bytes, iv: &[u8]) -> Result<Bytes> {
    if iv.len() != 16 {
        return Err("IV must be 16 bytes for AES-256-CBC".into());
    }
    let key: &[u8; 32] = &*KEY;

    // Prepare buffer with extra capacity for PKCS7 padding (max one block)
    let mut buf = data.to_vec();
    let msg_len = buf.len();
    let block_size = 16; // AES block size in bytes
    buf.resize(msg_len + block_size, 0u8);

    let cipher = Aes256Cbc::new_from_slices(key, iv)?;

    let out = cipher
        .encrypt_padded_mut::<Pkcs7>(&mut buf, msg_len)
        .map_err(|e| -> crate::err::Error {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Padding failed due to {}", e),
            ))
        })?;

    Ok(Bytes::copy_from_slice(out))
}

#[inline]
pub fn decrypt(cipher: Bytes, iv: &[u8]) -> Result<Bytes> {
    if iv.len() != 16 {
        return Err("IV must be 16 bytes for AES-256-CBC".into());
    }
    let key: &[u8; 32] = &*KEY;

    // Decrypt in-place and remove PKCS7 padding
    let mut buf = cipher.to_vec();
    let dec = Aes256CbcDec::new_from_slices(key, iv)?;
    let out = dec
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| -> crate::err::Error {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Unpadding failed due to {}", e),
            ))
        })?;

    Ok(Bytes::copy_from_slice(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::config::EnvVar;
    use crate::global_var::ENV_VAR;

    #[test]
    fn test_encrypt_decrypt_success() {
        // Ensure ENV_VAR is initialized so KEY derivation can access conn_token
        if ENV_VAR.get().is_none() {
            let mut cfg = Config::new();
            cfg.identity.machine_name = "test-machine".into();
            cfg.identity.private_key_loc = "~/.keys/priv".into();
            cfg.identity.public_key_loc = "~/.keys/pub".into();
            cfg.connection.conn_token = "TEST_TOKEN".into();
            cfg.app_config.working_dir = "~/disc_work".into();

            let ev = EnvVar::from_config(&cfg).expect("EnvVar::from_config should succeed");
            let _ = ENV_VAR.set(ev); // ignore if already set by other tests
        }

        let data = Bytes::from("hello world");
        let data_copy = data.clone();

        let iv = [0x8; 16];

        let encrypted = encrypt(data, &iv).unwrap();

        let decrypted = decrypt(encrypted, &iv).unwrap();

        assert_eq!(data_copy, decrypted);
    }
}
