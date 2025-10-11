use crate::err::Result;
use crate::global_var::LOGGER;
use crate::network::protocol::messages::HelloMessage;
use crate::tasks::Handleable;

impl Handleable for HelloMessage {
    fn handle(&mut self) -> Result<()> {
        LOGGER.info(format!("HelloMessage: {:?}", self));
        Ok(())
    }
}
