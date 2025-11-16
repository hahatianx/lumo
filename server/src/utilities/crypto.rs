use crate::err::Result;
use crate::global_var::ENV_VAR;
use crate::utilities::crypto;
use aes::Aes256;
use bincode::config;
use bytes::Bytes;
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use cbc::{Decryptor, Encryptor};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;
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

pub fn to_encryption<T, F>(data: &T, generate_iv: F) -> Result<Vec<u8>>
where
    T: Serialize,
    F: Fn() -> Result<[u8; 16]>,
{
    // Serialize self using bincode v2 serde API
    let cfg = config::standard();
    let raw_bytes = bincode::serde::encode_to_vec(data, cfg)?;

    let iv = generate_iv()?;

    // Encrypt the serialized payload using AES-256-CBC (PKCS7)
    let encrypted = encrypt(Bytes::from(raw_bytes), &iv)?;

    // Prepend IV in clear so the receiver can decrypt
    let mut out: Vec<u8> = Vec::with_capacity(16 + encrypted.len());
    out.extend_from_slice(&iv);
    out.extend_from_slice(&encrypted);
    Ok(out)
}

pub fn from_encryption<T>(ciphertext: Box<[u8]>) -> Result<T>
where
    T: DeserializeOwned,
{
    if ciphertext.len() < 16 {
        return Err("Incorrect cipher: IV too short".into());
    }
    let iv: [u8; 16] = ciphertext[..16]
        .try_into()
        .map_err(|_| "Incorrect cipher: IV too short")?;

    let decrypted = crypto::decrypt(Bytes::from(ciphertext[16..].to_vec()), &iv)?;

    let raw_bytes = decrypted.to_vec();
    let (message, _) = bincode::serde::decode_from_slice(&raw_bytes, config::standard())?;

    Ok(message)
}

pub fn f_to_encryption<P: AsRef<Path>, F>(from_path: P, to_path: P, generate_iv: F) -> Result<()>
where
    F: Fn() -> Result<[u8; 16]>,
{
    use aes::cipher::generic_array::GenericArray;
    use aes::cipher::{BlockEncrypt, KeyInit};

    let from = from_path.as_ref();
    let to = to_path.as_ref();

    // 1) Validate paths
    if !from.exists() {
        return Err("from_path does not exist".into());
    }
    let meta = fs::metadata(from)?;
    if !meta.is_file() {
        return Err("from_path must be a regular file".into());
    }
    if to.exists() {
        return Err("to_path must not exist".into());
    }

    // 2) Open files
    let infile = std::fs::File::open(from)?;
    let mut reader = BufReader::new(infile);
    let outfile = OpenOptions::new().write(true).create_new(true).open(to)?;
    let mut writer = BufWriter::new(outfile);

    // 3) Prepare crypto: key, IV and AES-256 block cipher
    let key: &[u8; 32] = &*KEY;
    let cipher = Aes256::new(GenericArray::from_slice(key));
    let iv = generate_iv()?;

    // Write IV in clear at the beginning
    writer.write_all(&iv)?;

    // 4) Stream encrypt in CBC with PKCS7 padding
    let mut prev = iv;
    let mut carry: Vec<u8> = Vec::with_capacity(16);
    let mut buf = [0u8; 64 * 1024];

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        carry.extend_from_slice(&buf[..n]);

        // Process all full 16-byte blocks, leave remainder in carry
        let mut to_process_len = (carry.len() / 16) * 16;
        // Defer padding to EOF; here we process all full blocks
        let mut i = 0;
        while i + 16 <= to_process_len {
            let mut block = [0u8; 16];
            block.copy_from_slice(&carry[i..i + 16]);
            // XOR with prev (CBC)
            for j in 0..16 {
                block[j] ^= prev[j];
            }
            // Encrypt single block
            let mut b = GenericArray::from(block);
            cipher.encrypt_block(&mut b);
            // Write ciphertext and update prev
            writer.write_all(b.as_slice())?;
            prev.copy_from_slice(b.as_slice());
            i += 16;
        }
        // Remove processed bytes from carry efficiently
        if i > 0 {
            carry.drain(0..i);
        }
    }

    // 5) Final padding block (PKCS7)
    let pad_len = 16 - (carry.len() % 16);
    carry.extend(std::iter::repeat(pad_len as u8).take(pad_len));
    debug_assert_eq!(carry.len() % 16, 0);

    // Now carry may be 16 bytes (when input was multiple of 16, it was empty and we just appended a full padding block) or >16 if small remainder existed
    let mut i = 0;
    while i < carry.len() {
        let mut block = [0u8; 16];
        block.copy_from_slice(&carry[i..i + 16]);
        for j in 0..16 {
            block[j] ^= prev[j];
        }
        let mut b = GenericArray::from(block);
        cipher.encrypt_block(&mut b);
        writer.write_all(b.as_slice())?;
        prev.copy_from_slice(b.as_slice());
        i += 16;
    }

    writer.flush()?;
    Ok(())
}

pub fn f_from_encryption<P: AsRef<Path>>(from_path: P, to_path: P) -> Result<()> {
    use aes::cipher::generic_array::GenericArray;
    use aes::cipher::{BlockDecrypt, KeyInit};

    let from = from_path.as_ref();
    let to = to_path.as_ref();

    // 1) Validate paths
    if !from.exists() {
        return Err("from_path does not exist".into());
    }
    let meta = fs::metadata(from)?;
    if !meta.is_file() {
        return Err("from_path must be a regular file".into());
    }
    if to.exists() {
        return Err("to_path must not exist".into());
    }

    // 2) Open files
    let infile = std::fs::File::open(from)?;
    let mut reader = BufReader::new(infile);
    let outfile = OpenOptions::new().write(true).create_new(true).open(to)?;
    let mut writer = BufWriter::new(outfile);

    // 3) Read IV (first 16 bytes)
    let mut iv = [0u8; 16];
    reader.read_exact(&mut iv)?;

    // 4) Prepare cipher
    let key: &[u8; 32] = &*KEY;
    let cipher = Aes256::new(GenericArray::from_slice(key));

    // 5) Stream decrypt with CBC and PKCS7 unpadding
    let mut prev_ct = iv; // previous ciphertext block (IV initially)
    let mut carry: Vec<u8> = Vec::with_capacity(64 * 1024);
    let mut buf = [0u8; 64 * 1024];
    let mut eof = false;

    // Helper to process one plaintext block (not the last padded one)
    let mut process_block = |block_ct: &[u8; 16], prev: &mut [u8; 16]| -> Result<()> {
        let mut ga = GenericArray::clone_from_slice(block_ct);
        cipher.decrypt_block(&mut ga);
        // XOR with prev
        let mut plain_block = [0u8; 16];
        for i in 0..16 {
            plain_block[i] = ga[i] ^ prev[i];
        }
        writer.write_all(&plain_block)?;
        prev.copy_from_slice(block_ct);
        Ok(())
    };

    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            eof = true;
        }
        carry.extend_from_slice(&buf[..n]);

        // While we have at least two blocks in carry, we can process the first block safely
        while carry.len() >= 32 {
            let block_ct: [u8; 16] = carry[0..16].try_into().unwrap();
            process_block(&block_ct, &mut prev_ct)?;
            carry.drain(0..16);
        }

        if eof {
            break;
        }
    }

    // At EOF, carry must contain a whole number of blocks and at least one block
    if carry.len() == 0 || (carry.len() % 16 != 0) {
        return Err("Ciphertext truncated or invalid (after IV)".into());
    }

    // If there are more than one blocks, process all but the last
    while carry.len() > 16 {
        let block_ct: [u8; 16] = carry[0..16].try_into().unwrap();
        process_block(&block_ct, &mut prev_ct)?;
        carry.drain(0..16);
    }

    debug_assert_eq!(carry.len(), 16);
    let last_block_ct: [u8; 16] = carry[0..16].try_into().unwrap();

    // Decrypt last block and remove PKCS7 padding
    let mut ga = GenericArray::clone_from_slice(&last_block_ct);
    cipher.decrypt_block(&mut ga);
    let mut last_plain = [0u8; 16];
    for i in 0..16 {
        last_plain[i] = ga[i] ^ prev_ct[i];
    }

    // Validate PKCS7
    let pad_len = last_plain[15] as usize;
    if pad_len == 0 || pad_len > 16 {
        return Err("Invalid PKCS7 padding".into());
    }
    for i in 0..pad_len {
        if last_plain[15 - i] as usize != pad_len {
            return Err("Invalid PKCS7 padding".into());
        }
    }
    let data_len = 16 - pad_len;
    writer.write_all(&last_plain[..data_len])?;

    writer.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::config::EnvVar;
    use crate::global_var::ENV_VAR;
    use std::fs;
    use std::path::PathBuf;

    fn ensure_env() {
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
    }

    fn tmp_path(name: &str) -> PathBuf {
        let mut p = std::env::temp_dir();
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        p.push(format!("disc_proj_crypto_test_{}_{}", name, nanos));
        p
    }

    #[test]
    fn test_encrypt_decrypt_success() {
        ensure_env();

        let data = Bytes::from("hello world");
        let data_copy = data.clone();

        let iv = [0x8; 16];

        let encrypted = encrypt(data, &iv).unwrap();

        let decrypted = decrypt(encrypted, &iv).unwrap();

        assert_eq!(data_copy, decrypted);
    }

    #[test]
    fn test_file_encrypt_decrypt_roundtrip_various_sizes() {
        ensure_env();
        let sizes = [0usize, 1, 15, 16, 17, 100_000];
        let iv = [9u8; 16];

        for &sz in &sizes {
            let from = tmp_path(&format!("plain_{}", sz));
            let enc = tmp_path(&format!("enc_{}", sz));
            let dec = tmp_path(&format!("dec_{}", sz));

            // prepare plaintext file
            let mut plain = Vec::with_capacity(sz);
            for i in 0..sz {
                plain.push((i % 251) as u8);
            }
            fs::write(&from, &plain).unwrap();

            // encrypt to file with fixed IV
            f_to_encryption(&from, &enc, || Ok(iv)).unwrap();
            assert!(enc.exists());

            // verify IV prefix
            let mut f = fs::File::open(&enc).unwrap();
            let mut iv_read = [0u8; 16];
            f.read_exact(&mut iv_read).unwrap();
            assert_eq!(iv_read, iv);

            // decrypt back
            f_from_encryption(&enc, &dec).unwrap();
            let round = fs::read(&dec).unwrap();
            assert_eq!(round, plain);

            // cleanup
            let _ = fs::remove_file(&from);
            let _ = fs::remove_file(&enc);
            let _ = fs::remove_file(&dec);
        }
    }

    #[test]
    fn test_to_path_must_not_exist() {
        ensure_env();
        let from = tmp_path("plain_exist");
        let enc = tmp_path("enc_exist");
        fs::write(&from, b"abc").unwrap();
        fs::write(&enc, b"already there").unwrap();

        let iv = [7u8; 16];
        let err = f_to_encryption(&from, &enc, || Ok(iv))
            .err()
            .expect("should error");
        let msg = format!("{}", err);
        assert!(msg.contains("must not exist"));

        let _ = fs::remove_file(&from);
        let _ = fs::remove_file(&enc);
    }

    #[test]
    fn test_from_path_must_exist_and_be_file() {
        ensure_env();
        let from = tmp_path("missing");
        let enc = tmp_path("out");
        // from does not exist
        let err = f_to_encryption(&from, &enc, || Ok([1u8; 16]))
            .err()
            .expect("should error");
        assert!(format!("{}", err).contains("does not exist"));
    }
}
