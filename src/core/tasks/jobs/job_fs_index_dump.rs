use crate::core::tasks::jobs::JobClosure;
use crate::err::Result;
use crate::fs::FS_INDEX;
use crate::global_var::LOGGER;
use std::sync::LazyLock;
use tokio::sync::RwLock;

static LAST_CHECKSUM: LazyLock<RwLock<Option<u64>>> = LazyLock::new(|| RwLock::new(None));
pub async fn get_job_fs_index_dump_closure() -> Result<Box<JobClosure>> {
    let closure = move || {
        let fut: std::pin::Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>> =
            Box::pin(async move {
                let mut last_index_dump_checksum = LAST_CHECKSUM.write().await;
                match FS_INDEX.dump_index(*last_index_dump_checksum).await {
                    Ok(checksum) => {
                        last_index_dump_checksum.replace(checksum);
                    }
                    Err(_e) => {}
                };
                LOGGER.trace(format!(
                    "[fs dump] Latest dumped index checksum: lass_index_dump_checksum: {:?} ",
                    last_index_dump_checksum
                        .as_ref()
                        .map(|c| format!("0x{:x}", c))
                ));
                Ok(())
            });
        fut
    };

    Ok(Box::new(closure))
}
