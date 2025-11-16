use crate::core::tasks::AsyncHandleable;
use crate::types::Expected;
use async_trait::async_trait;

type Nonce = u64;
type Checksum = u64;

pub struct DownloadFileTask {
    nonce: Nonce,
    expected_checksum: Expected<Checksum>,
}

impl DownloadFileTask {
    pub fn new(nonce: Nonce, expected_checksum: Option<Checksum>) -> Self {
        Self {
            nonce,
            expected_checksum: expected_checksum.into(),
        }
    }
}

#[async_trait]
impl AsyncHandleable for DownloadFileTask {
    async fn handle(&mut self) -> crate::err::Result<()> {
        Ok(())
    }
}
