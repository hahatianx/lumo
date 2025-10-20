use std::net::UdpSocket;
use std::time::{Duration, SystemTime};
use api_model::protocol::message::api_request_message::{ApiRequestKind, ApiRequestMessage};
use api_model::protocol::message::api_response_message::ApiResponseMessage;
use api_model::protocol::models::list_peers::ListPeersRequest;
use api_model::protocol::protocol::Protocol;

pub fn list_peers() {
    let start_time = SystemTime::now();

    // Create a UDP socket bound to an ephemeral local port in blocking mode
    // Blocking is the default, but we explicitly set it for clarity
    match UdpSocket::bind("0.0.0.0:0") {
        Ok(socket) => {
            // ensure blocking mode
            if let Err(e) = socket.set_nonblocking(false) {
                eprintln!("Failed to set blocking mode on UDP socket: {}", e);
                return;
            }
            // Optionally set a reasonable read timeout so the CLI doesn't hang forever
            // let _ = socket.set_read_timeout(Some(Duration::from_secs(5)));

            println!(
                "UDP socket created (blocking). Local addr: {}",
                socket.local_addr().map(|a| a.to_string()).unwrap_or_else(|_| "<unknown>".to_string())
            );

            let payload = ApiRequestMessage::new(
                socket.local_addr().unwrap().ip().to_string(),
                socket.local_addr().unwrap().port(),
                ApiRequestKind::ListPeers(ListPeersRequest),
            ).serialize();

            socket.send_to(payload.as_ref(), "127.0.0.1:14514").unwrap();

            let mut buf = [0; 1024];
            let response = socket.recv_from(&mut buf).unwrap();

            let response_message = ApiResponseMessage::deserialize(&buf[..response.0]).unwrap();

            println!("Received response: {:?}", response_message);

        }
        Err(e) => {
            eprintln!("Failed to create UDP socket: {}", e);
            return;
        }
    }

    let _end_time = SystemTime::now();

    println!("Time elapsed: {:?}", _end_time.duration_since(start_time).unwrap());
}
