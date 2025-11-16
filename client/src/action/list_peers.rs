use crate::action::conn::Connection;
use crate::error::ClientError;
use api_model::protocol::message::api_request_message::ApiRequestKind;
use cli_handler::cli_impl;
use api_model::protocol::models::peer::list_peers::ListPeersRequest;

#[cli_impl]
pub fn list_peers() -> Result<(), ClientError> {
    let conn = Connection::new(None)?;

    let res = conn.request(ApiRequestKind::ListPeers(ListPeersRequest))?;
    println!("{:?}", res);

    Ok(())
}
