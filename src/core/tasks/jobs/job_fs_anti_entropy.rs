use crate::err::Result;
use crate::fs::FS_INDEX;

pub async fn job_fs_anti_entropy() -> Result<()> {
    FS_INDEX.index_stale_rescan().await
}