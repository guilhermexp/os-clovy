#![allow(clippy::expect_used)]

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

pub(crate) async fn start_charge_server_with_responses<const N: usize>(
    responses: [Vec<u8>; N],
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let addr = listener.local_addr().expect("read local address");
    tokio::spawn(async move {
        for response in responses {
            let mut stream = loop {
                let (mut stream, _) = listener.accept().await.expect("accept request");
                if read_charge_request(&mut stream).await {
                    break stream;
                }
                // Local discovery probes must not consume a scripted receipt.
                // A probe may disconnect without reading its rejection.
                let _ = stream
                    .write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    )
                    .await;
                let _ = stream.shutdown().await;
            };
            stream.write_all(&response).await.expect("write response");
            stream.shutdown().await.expect("close response");
        }
    });
    format!("http://{addr}")
}

/// Reads one whole request and checks that it is `POST /charge`. Closing a
/// socket with unread request bytes makes the kernel answer with a reset,
/// which can discard the scripted response before the client reads it.
async fn read_charge_request(stream: &mut TcpStream) -> bool {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let Ok(read) = stream.read(&mut buffer).await else {
            return false;
        };
        if read == 0 {
            return false;
        }
        request.extend_from_slice(&buffer[..read]);
        let Some(headers_end) = request.windows(4).position(|window| window == b"\r\n\r\n") else {
            continue;
        };
        let body_len = String::from_utf8_lossy(&request[..headers_end])
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                if name.eq_ignore_ascii_case("content-length") {
                    value.trim().parse::<usize>().ok()
                } else {
                    None
                }
            })
            .unwrap_or(0);
        if request.len() >= headers_end + 4 + body_len {
            return request.starts_with(b"POST /charge HTTP/1.1\r\n");
        }
    }
}
