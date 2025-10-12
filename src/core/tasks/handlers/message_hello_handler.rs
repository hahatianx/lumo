use crate::core::tasks::Handleable;
use crate::err::Result;
use crate::global_var::LOGGER;
use crate::network::protocol::messages::HelloMessage;

impl Handleable for HelloMessage {
    fn handle(&mut self) -> Result<()> {
        LOGGER.info(format!("HelloMessage: {:?}", self));
        Ok(())
    }
}
