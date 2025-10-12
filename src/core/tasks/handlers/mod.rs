mod message_hello_handler;

use crate::err::Result;

pub trait Handleable {
    fn handle(&mut self) -> Result<()>;
}

pub type AsyncHandleable = dyn Handleable + Send + 'static;
