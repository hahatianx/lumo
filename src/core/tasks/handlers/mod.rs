mod message_hello_handler;
use async_trait::async_trait;

use crate::err::Result;

#[async_trait]
pub trait AsyncHandleable: Send + 'static {
    async fn handle(&mut self) -> Result<()>;
}
