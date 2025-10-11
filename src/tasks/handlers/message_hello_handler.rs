use crate::tasks::Handleable;
use crate::err::Result;
use crate::network::protocol::messages::HelloMessage;

impl Handleable for HelloMessage {
    fn handle(&mut self) -> Result<()> {
        println!("HelloMessage: {:?}", self);
        Ok(())
    }
}