use crate::err::Result;
use crate::global_var::{ENV_VAR, LOGGER};
use rand::random;
use serde::Deserialize;
use std::path::PathBuf;

type ByteDeserializer<'a, C> = fn(&[u8]) -> Result<C>;

pub async fn fs_save_bytes_atomic_internal(dest: &PathBuf, data: &[u8]) -> Result<()> {
    let temp_dir = PathBuf::from(ENV_VAR.get().unwrap().get_working_dir())
        .join(".disc")
        .join("tmp_downloads");

    let tmp = temp_dir.join(format!("index.tmp-{}", random::<u64>()));
    tokio::fs::write(&tmp, data).await?;
    tokio::fs::rename(&tmp, dest).await?;

    LOGGER.trace(format!("fs_save_bytes_internal: saved to {}", dest.display()).as_str());

    Ok(())
}

pub async fn fs_read_bytes_deserialized<'a, C: Deserialize<'a>>(
    src: &PathBuf,
    deserializer: ByteDeserializer<'a, C>,
) -> Result<C> {
    let bytes = tokio::fs::read(&src).await?;
    let deserialized = deserializer(&bytes)?;
    Ok(deserialized)
}
