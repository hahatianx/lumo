use crate::global_var::ENV_VAR;
use crate::utilities::crypto::{from_encryption, to_encryption};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

type Nonce = u64;
type Checksum = u64;

#[derive(Debug)]
pub enum FileSyncError {
    AbortedByPeer,
    Timeout,
    FileMalformed,
    SystemError,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSync {
    nonce: Nonce,
    timestamp: SystemTime,
}

impl FileSync {
    pub fn new(nonce: Nonce) -> Self {
        Self {
            nonce,
            timestamp: SystemTime::now(),
        }
    }

    pub fn nonce(&self) -> Nonce {
        self.nonce
    }

    pub fn to_encryption(&self) -> crate::err::Result<Vec<u8>> {
        to_encryption(&self, || {
            let iv = rand::random::<[u8; 16]>();
            Ok(iv)
        })
    }

    pub fn from_encryption(ciphertext: Box<[u8]>) -> crate::err::Result<Self> {
        from_encryption(ciphertext)
    }

    pub fn is_valid(&self) -> bool {
        let current_time = SystemTime::now();
        let diff = current_time
            .duration_since(self.timestamp)
            .unwrap_or(std::time::Duration::from_secs(0));
        diff.as_secs() < ENV_VAR.get().unwrap().get_pull_task_validity_in_sec()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSyncAck {
    nonce: Nonce,
    maybe_checksum: Option<u64>,
    timestamp: SystemTime,
    file_size: u64, // in bytes
}

impl FileSyncAck {
    pub fn new(nonce: Nonce, maybe_checksum: Option<Checksum>, file_size: u64) -> Self {
        Self {
            nonce,
            maybe_checksum,
            timestamp: SystemTime::now(),
            file_size,
        }
    }

    #[inline]
    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    #[inline]
    pub fn maybe_checksum(&self) -> Option<u64> {
        self.maybe_checksum
    }

    #[inline]
    pub fn nonce(&self) -> Nonce {
        self.nonce
    }

    pub fn to_encryption(&self) -> crate::err::Result<Vec<u8>> {
        to_encryption(&self, || {
            let iv = rand::random::<[u8; 16]>();
            Ok(iv)
        })
    }

    pub fn from_encryption(ciphertext: Box<[u8]>) -> crate::err::Result<Self> {
        from_encryption(ciphertext)
    }
}
