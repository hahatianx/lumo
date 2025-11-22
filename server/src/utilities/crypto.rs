use crate::err::Result;
use crate::fs::fs_lock;
use crate::global_var::{ENV_VAR, LOGGER};
use crate::utilities::crypto;
use aes::Aes256;
use age::secrecy::SecretString;
use bincode::config;
use bytes::Bytes;
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};
use cbc::{Decryptor, Encryptor};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
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

fn identity_from_password(password: &str, salt: &str) -> Result<age::x25519::Identity> {
    // Derive a 32-byte secret from password+salt via SHA-256
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    hasher.update(salt.as_bytes());
    let hash = hasher.finalize();

    // Encode as proper age bech32 secret key string with HRP "age-secret-key-"
    let sk: [u8; 32] = hash[..32].try_into().unwrap();
    let sk_b32 = bech32::ToBase32::to_base32(&sk);
    let sk_bech32 = bech32::encode("age-secret-key-", sk_b32, bech32::Variant::Bech32)?;

    Ok(sk_bech32.parse::<age::x25519::Identity>()?)
}

pub async fn f_to_encryption<P: AsRef<Path>>(
    from_path: P,
    to_path: P,
    passphrase: &str,
) -> Result<()> {
    let from = from_path.as_ref();
    let to = to_path.as_ref();

    if !crate::utilities::disk_op::check_path_inbound(from) {
        return Err("from_path is not in working directory".into());
    }
    if !crate::utilities::disk_op::check_path_inbound(to) {
        return Err("to_path is not in working directory".into());
    }

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

    let identity = identity_from_password(ENV_VAR.get().unwrap().get_conn_token(), passphrase)?;
    let recipient = identity.to_public();
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))?;

    // 2) Open files
    let infile = &*fs_lock::RwLock::new(from).read().await?;
    let mut reader = BufReader::new(infile);
    let outfile = OpenOptions::new().write(true).create_new(true).open(to)?;
    let mut writer = encryptor.wrap_output(BufWriter::new(outfile))?;

    LOGGER.trace(format!("Encrypting {} to {}", from.display(), to.display()).as_str());
    std::io::copy(&mut reader, &mut writer)?;
    writer.finish()?;
    LOGGER.trace(format!("Completed encrypting {} to {}", from.display(), to.display()).as_str());
    Ok(())
}

pub async fn f_from_encryption<P: AsRef<Path>>(
    from_path: P,
    to_path: P,
    passphrase: &str,
) -> Result<()> {
    let from = from_path.as_ref();
    let to = to_path.as_ref();

    if !crate::utilities::disk_op::check_path_inbound(from) {
        return Err("from_path is not in working directory".into());
    }
    if !crate::utilities::disk_op::check_path_inbound(to) {
        return Err("to_path is not in working directory".into());
    }

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
    let infile = &*fs_lock::RwLock::new(from).read().await?;
    let outfile = OpenOptions::new().write(true).create_new(true).open(to)?;
    let mut writer = BufWriter::new(outfile);

    let identity = identity_from_password(ENV_VAR.get().unwrap().get_conn_token(), passphrase)?;
    let decryptor = age::Decryptor::new(infile)?;
    let mut reader = decryptor.decrypt(std::iter::once(&identity as &dyn age::Identity))?;

    std::io::copy(&mut reader, &mut writer)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::config::EnvVar;
    use crate::global_var::ENV_VAR;
    use crate::utilities::temp_dir::TmpDirGuard;
    use std::fs;
    use std::path::PathBuf;

    fn ensure_env() {
        if ENV_VAR.get().is_none() {
            let mut cfg = Config::new();
            cfg.identity.machine_name = "test-machine".into();
            cfg.identity.private_key_loc = "~/.keys/priv".into();
            cfg.identity.public_key_loc = "~/.keys/pub".into();
            cfg.connection.conn_token = "TEST_TOKEN".into();

            // Use a temp working directory that actually exists, so inbound checks pass
            let mut wd = std::env::temp_dir();
            wd.push("disc_proj_workdir_for_tests");
            let _ = fs::create_dir_all(&wd);
            cfg.app_config.working_dir = wd.to_string_lossy().to_string();

            let ev = EnvVar::from_config(&cfg).expect("EnvVar::from_config should succeed");
            let _ = ENV_VAR.set(ev); // ignore if already set by other tests
        }
    }

    // Create a dedicated temporary workspace directory under the configured working_dir.
    // All test files are created inside this directory so the guard can remove everything
    // on drop, even if the test panics midway.
    fn tmp_dir(prefix: &str) -> TmpDirGuard {
        use std::path::PathBuf;
        // Prefer the working_dir from ENV_VAR to satisfy inbound checks, fallback to temp
        let mut base: PathBuf = if let Some(ev) = ENV_VAR.get() {
            PathBuf::from(ev.get_working_dir())
        } else {
            panic!("ensure_env must be called before tmp_dir");
        };
        let _ = fs::create_dir_all(&base);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        base.push(format!("disc_proj_crypto_ws_{}_{}", prefix, nanos));
        let _ = fs::create_dir_all(&base);
        TmpDirGuard::from(base)
    }

    // Build a path inside the given workspace directory
    fn in_dir(dir: &TmpDirGuard, name: &str) -> PathBuf {
        dir.join(name)
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

    #[tokio::test]
    async fn test_file_encrypt_decrypt_roundtrip_various_sizes() {
        ensure_env();
        let sizes = [0usize, 1, 15, 16, 17, 100_000];

        for &sz in &sizes {
            // Per-iteration isolated workspace so each size cleans up independently
            let ws = tmp_dir("roundtrip");
            let from = in_dir(&ws, &format!("plain_{}", sz));
            let enc = in_dir(&ws, &format!("enc_{}", sz));
            let dec = in_dir(&ws, &format!("dec_{}", sz));

            // prepare plaintext file
            let mut plain = Vec::with_capacity(sz);
            for i in 0..sz {
                plain.push((i % 251) as u8);
            }
            fs::write(&from, &plain).unwrap();
            let passphrase = "QAQ";

            // encrypt to file
            f_to_encryption(&from, &enc, &passphrase).await.unwrap();
            assert!(enc.exists());

            // decrypt back
            f_from_encryption(&enc, &dec, &passphrase).await.unwrap();
            let round = fs::read(&dec).unwrap();
            assert_eq!(round, plain);

            // No manual cleanup: ws guard removes the whole directory on drop
        }
    }

    #[tokio::test]
    async fn test_to_path_must_not_exist() {
        ensure_env();
        let ws = tmp_dir("must_not_exist");
        let from = in_dir(&ws, "plain_exist");
        let enc = in_dir(&ws, "enc_exist");
        fs::write(&from, b"abc").unwrap();
        fs::write(&enc, b"already there").unwrap();

        let passphrase = "QAQ";

        let err = f_to_encryption(&from, &enc, &passphrase).await;
        if let Ok(()) = err {
            panic!("should error");
        }
        let msg = format!("{}", err.err().unwrap());
        assert!(msg.contains("must not exist"));
        // No manual cleanup; ws guard removes the directory
    }

    #[tokio::test]
    async fn test_from_path_must_exist_and_be_file() {
        ensure_env();
        let ws = tmp_dir("missing_input");
        let from = in_dir(&ws, "missing");
        let enc = in_dir(&ws, "out");
        // from does not exist
        let passphrase = "QAQ";
        let err = f_to_encryption(&from, &enc, &passphrase).await;
        if let Ok(()) = err {
            panic!("should error");
        }
        assert!(format!("{}", err.err().unwrap()).contains("does not exist"));
    }

    #[tokio::test]
    async fn test_decrypt_with_wrong_passphrase_fails() {
        ensure_env();
        // prepare temporary workspace and files
        let ws = tmp_dir("wrong_pw");
        let from = in_dir(&ws, "plain_wrong_pw");
        let enc = in_dir(&ws, "enc_wrong_pw");
        let dec = in_dir(&ws, "dec_wrong_pw");

        // write some plaintext
        fs::write(&from, b"The quick brown fox jumps over the lazy dog").unwrap();

        // encrypt with passphrase A
        let passphrase_ok = "correct horse battery staple";
        f_to_encryption(&from, &enc, passphrase_ok).await.unwrap();
        assert!(enc.exists());

        // try decrypt with passphrase B (wrong)
        let passphrase_bad = "wrong passphrase";
        let res = f_from_encryption(&enc, &dec, passphrase_bad).await;
        assert!(res.is_err(), "decryption should fail with wrong passphrase");
        // No manual cleanup: ws guard removes the directory
    }
}
