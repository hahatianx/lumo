use api_model::protocol::message::api_request_message::{ApiRequestKind, ApiRequestMessage};
use api_model::protocol::message::api_response_message::ApiResponseMessage;
use api_model::protocol::models::local_pull_file::LocalPullFileRequest;
use api_model::protocol::protocol::Protocol;
use std::net::UdpSocket;
use std::time::SystemTime;

pub fn local_pull_file(src_file_path: &str, expected_checksum: Option<u64>) {
    let start_time = SystemTime::now();

    match UdpSocket::bind("0.0.0.0:0") {
        Ok(socket) => {
            if let Err(e) = socket.set_nonblocking(false) {
                eprintln!("Failed to set blocking mode on UDP socket: {}", e);
                return;
            }

            let payload = ApiRequestMessage::new(
                socket.local_addr().unwrap().ip().to_string(),
                socket.local_addr().unwrap().port(),
                ApiRequestKind::LocalPullFile(LocalPullFileRequest {
                    path: src_file_path.to_string(),
                    expected_checksum,
                }),
            )
            .serialize();

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

    let end_time = SystemTime::now();
    println!(
        "Time elapsed: {:?}",
        end_time.duration_since(start_time).unwrap()
    );
}
