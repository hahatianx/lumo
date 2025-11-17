use crate::global_var::LOGGER;
use std::ops::Deref;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct TmpDirGuard(pub PathBuf);
impl Drop for TmpDirGuard {
    fn drop(&mut self) {
        LOGGER.trace(format!(
            "TmpDirGuard dropping, removing temporary directory: {:?}",
            &self.0
        ));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl From<PathBuf> for TmpDirGuard {
    fn from(path: PathBuf) -> Self {
        Self(path)
    }
}

impl AsRef<PathBuf> for TmpDirGuard {
    fn as_ref(&self) -> &PathBuf {
        &self.0
    }
}

impl AsRef<Path> for TmpDirGuard {
    fn as_ref(&self) -> &Path {
        self.0.as_path()
    }
}

impl Deref for TmpDirGuard {
    type Target = PathBuf;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
