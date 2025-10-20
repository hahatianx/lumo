use crate::err::Result;
use crate::fs::FS_INDEX;

pub async fn job_fs_stale_rescan() -> Result<()> {
    FS_INDEX.index_stale_rescan().await
}

pub async fn job_fs_inactive_cleanup() -> Result<()> {
    FS_INDEX.index_inactive_clean().await
}
