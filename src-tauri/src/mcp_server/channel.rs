//! The local channel between `clovy-mcp` (spawned by an MCP client) and the
//! running app: a Unix socket in `<app data dir>/mcp/` (directory 0700,
//! socket 0600) and an installation secret in the same directory (file
//! 0600). The binary never opens Clovy's databases; it relays JSON-RPC lines
//! here.
//!
//! Wire format: newline-delimited JSON. The client's first line is the
//! handshake `{"clovyMcp": 1, "secret": "<hex>"}`; the app answers
//! `{"clovyMcp": 1, "ok": true}`, or `{"clovyMcp": 1, "ok": false, "error":
//! "unauthorized"}` (or `"busy"` when every connection slot is taken) and
//! closes. After `ok`, each line is one MCP JSON-RPC message in either
//! direction; responses may arrive out of order.
//!
//! Limits (every process of the same user can reach the socket): at most
//! `MAX_PENDING_HANDSHAKES` connections waiting for their handshake (more are
//! closed as soon as they are accepted), `MAX_CONNECTIONS` authenticated
//! connections, and `MAX_IN_FLIGHT_PER_CONNECTION` requests being answered
//! per connection (the app stops reading that connection until one finishes).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

pub const DIR_NAME: &str = "mcp";
pub const SOCKET_FILE: &str = "clovy.sock";
pub const SECRET_FILE: &str = "secret";
pub const HANDSHAKE_VERSION: u64 = 1;
/// Longest accepted line, in bytes (a JSON-RPC message).
pub const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;
/// Accepted connections still in the handshake at once.
pub const MAX_PENDING_HANDSHAKES: usize = 4;
/// Authenticated connections at once (each MCP client holds one).
pub const MAX_CONNECTIONS: usize = 8;
/// Requests of one connection being answered at once.
pub const MAX_IN_FLIGHT_PER_CONNECTION: usize = 4;
const SECRET_BYTES: usize = 32;

/// `<app data dir>/mcp`, the directory the configuration snippets name.
pub fn channel_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(DIR_NAME)
}

pub fn socket_path(dir: &Path) -> PathBuf {
    dir.join(SOCKET_FILE)
}

pub fn secret_path(dir: &Path) -> PathBuf {
    dir.join(SECRET_FILE)
}

pub fn handshake(secret: &str) -> Value {
    json!({ "clovyMcp": HANDSHAKE_VERSION, "secret": secret })
}

fn valid_secret(secret: &str) -> bool {
    secret.len() == SECRET_BYTES * 2 && secret.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Equal-time comparison, so a wrong guess learns nothing from timing.
fn secrets_match(expected: &str, offered: &str) -> bool {
    expected.len() == offered.len()
        && expected
            .bytes()
            .zip(offered.bytes())
            .fold(0u8, |diff, (a, b)| diff | (a ^ b))
            == 0
}

/// Reads the installation secret (the relay side).
pub fn read_secret(dir: &Path) -> std::io::Result<String> {
    let secret = std::fs::read_to_string(secret_path(dir))?
        .trim()
        .to_string();
    if valid_secret(&secret) {
        Ok(secret)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "the Clovy MCP secret file is malformed",
        ))
    }
}

#[cfg(unix)]
pub use unix::{connect, ClientError, Handler, Listener};

#[cfg(unix)]
mod unix {
    use std::future::Future;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::sync::Arc;
    use std::time::Duration;

    use rand::RngCore;
    use serde_json::{json, Value};
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader as AsyncBufReader};
    use tokio::net::{UnixListener, UnixStream};
    use tokio::sync::{mpsc, watch, OwnedSemaphorePermit, Semaphore};

    use super::*;

    const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
    const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(100);

    /// Answers one MCP message (`None` for notifications).
    pub type Handler =
        Arc<dyn Fn(Value) -> Pin<Box<dyn Future<Output = Option<Value>> + Send>> + Send + Sync>;

    fn ensure_private_dir(dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
    }

    /// The installation secret, created on first use (file mode 0600).
    pub(crate) fn load_or_create_secret(dir: &Path) -> std::io::Result<String> {
        ensure_private_dir(dir)?;
        match read_secret(dir) {
            Ok(secret) => return Ok(secret),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => std::fs::remove_file(secret_path(dir))?,
        }
        let mut bytes = [0u8; SECRET_BYTES];
        rand::thread_rng().fill_bytes(&mut bytes);
        let secret: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(secret_path(dir))?;
        file.write_all(secret.as_bytes())?;
        Ok(secret)
    }

    /// A running socket listener; dropping it without `stop` leaves the
    /// accept task running.
    pub struct Listener {
        socket: PathBuf,
        shutdown: watch::Sender<bool>,
    }

    impl Listener {
        /// Binds the socket and serves connections until `stop`. Must run
        /// inside the Tokio runtime.
        pub async fn start(dir: &Path, handler: Handler) -> std::io::Result<Self> {
            let secret: Arc<str> = load_or_create_secret(dir)?.into();
            let socket = socket_path(dir);
            if let Ok(metadata) = std::fs::symlink_metadata(&socket) {
                use std::os::unix::fs::FileTypeExt;
                if !metadata.file_type().is_socket() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        format!("{} exists and is not a socket", socket.display()),
                    ));
                }
                // Left behind by a previous run that did not stop cleanly.
                std::fs::remove_file(&socket)?;
            }
            let listener = UnixListener::bind(&socket).map_err(|error| {
                std::io::Error::new(
                    error.kind(),
                    format!("could not listen on {}: {error}", socket.display()),
                )
            })?;
            std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
            let (shutdown, mut stopped) = watch::channel(false);
            let connections = shutdown.subscribe();
            let handshakes = Arc::new(Semaphore::new(MAX_PENDING_HANDSHAKES));
            let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
            tokio::spawn(async move {
                loop {
                    tokio::select! {
                        accepted = listener.accept() => {
                            let stream = match accepted {
                                Ok((stream, _)) => stream,
                                Err(error) => {
                                    // Out of descriptors (EMFILE) and the like
                                    // persist: do not spin on them.
                                    tracing::warn!(%error, "mcp server: accept failed");
                                    tokio::time::sleep(ACCEPT_ERROR_BACKOFF).await;
                                    continue;
                                }
                            };
                            // Too many peers that have not authenticated yet:
                            // close this one now instead of holding it open.
                            let Ok(handshake) = Arc::clone(&handshakes).try_acquire_owned() else {
                                continue;
                            };
                            tokio::spawn(serve(
                                stream,
                                handshake,
                                Arc::clone(&slots),
                                Arc::clone(&secret),
                                Arc::clone(&handler),
                                connections.clone(),
                            ));
                        }
                        _ = stopped.changed() => break,
                    }
                }
            });
            Ok(Self { socket, shutdown })
        }

        pub fn socket(&self) -> &Path {
            &self.socket
        }

        /// Stops accepting, closes every open connection, and removes the
        /// socket file.
        pub fn stop(self) {
            let _ = self.shutdown.send(true);
            let _ = std::fs::remove_file(&self.socket);
        }
    }

    /// One line, at most `MAX_LINE_BYTES`; `None` at end of stream.
    async fn read_line<R: tokio::io::AsyncBufRead + Unpin>(
        reader: &mut R,
    ) -> std::io::Result<Option<Vec<u8>>> {
        let mut line = Vec::new();
        let read = reader
            .take(MAX_LINE_BYTES as u64 + 1)
            .read_until(b'\n', &mut line)
            .await?;
        if read == 0 {
            return Ok(None);
        }
        if line.len() > MAX_LINE_BYTES {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "message too long",
            ));
        }
        Ok(Some(line))
    }

    fn same_user(stream: &UnixStream) -> bool {
        // SAFETY: geteuid has no preconditions and cannot fail.
        let own = unsafe { libc::geteuid() };
        stream.peer_cred().is_ok_and(|peer| peer.uid() == own)
    }

    async fn serve(
        stream: UnixStream,
        handshake: OwnedSemaphorePermit,
        slots: Arc<Semaphore>,
        secret: Arc<str>,
        handler: Handler,
        mut shutdown: watch::Receiver<bool>,
    ) {
        let authorized = same_user(&stream);
        let (read, mut write) = stream.into_split();
        let mut reader = AsyncBufReader::new(read);
        let hello = tokio::time::timeout(HANDSHAKE_TIMEOUT, read_line(&mut reader)).await;
        let offered = match hello {
            Ok(Ok(Some(line))) => serde_json::from_slice::<Value>(&line).ok(),
            _ => None,
        };
        let accepted = authorized
            && offered.as_ref().is_some_and(|hello| {
                hello.get("clovyMcp").and_then(Value::as_u64) == Some(HANDSHAKE_VERSION)
                    && hello
                        .get("secret")
                        .and_then(Value::as_str)
                        .is_some_and(|offered| secrets_match(&secret, offered))
            });
        // Held until this connection ends.
        let slot = accepted.then(|| slots.try_acquire_owned().ok()).flatten();
        drop(handshake);
        let reply = match (accepted, &slot) {
            (true, Some(_)) => json!({ "clovyMcp": HANDSHAKE_VERSION, "ok": true }),
            (true, None) => json!({ "clovyMcp": HANDSHAKE_VERSION, "ok": false, "error": "busy" }),
            (false, _) => {
                json!({ "clovyMcp": HANDSHAKE_VERSION, "ok": false, "error": "unauthorized" })
            }
        };
        if write
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .is_err()
            || slot.is_none()
        {
            let _ = write.shutdown().await;
            return;
        }

        let (responses, mut outbox) = mpsc::channel::<Value>(MAX_IN_FLIGHT_PER_CONNECTION);
        let writer = tokio::spawn(async move {
            while let Some(response) = outbox.recv().await {
                if write
                    .write_all(format!("{response}\n").as_bytes())
                    .await
                    .is_err()
                {
                    break;
                }
            }
            let _ = write.shutdown().await;
        });
        let in_flight = Arc::new(Semaphore::new(MAX_IN_FLIGHT_PER_CONNECTION));
        loop {
            // Backpressure: the next line is read only once a request slot
            // is free, so a fast client waits in its own socket buffer.
            let permit = tokio::select! {
                permit = Arc::clone(&in_flight).acquire_owned() => match permit {
                    Ok(permit) => permit,
                    Err(_) => break,
                },
                _ = shutdown.changed() => break,
            };
            tokio::select! {
                line = read_line(&mut reader) => {
                    let Ok(Some(line)) = line else { break };
                    if line.iter().all(u8::is_ascii_whitespace) {
                        continue;
                    }
                    let responses = responses.clone();
                    let handler = Arc::clone(&handler);
                    tokio::spawn(async move {
                        let response = match serde_json::from_slice::<Value>(&line) {
                            Ok(message) => handler(message).await,
                            Err(error) => Some(crate::mcp_server::protocol::error_response(
                                Value::Null,
                                crate::mcp_server::protocol::PARSE_ERROR,
                                error.to_string(),
                            )),
                        };
                        drop(line);
                        if let Some(response) = response {
                            let _ = responses.send(response).await;
                        }
                        drop(permit);
                    });
                }
                _ = shutdown.changed() => break,
            }
        }
        if *shutdown.borrow() {
            writer.abort();
        } else {
            // The client closed its side: let pending answers drain.
            drop(responses);
            let _ = writer.await;
        }
        drop(slot);
    }

    /// Why the relay could not get an authenticated connection.
    #[derive(Debug)]
    pub enum ClientError {
        /// No secret yet: the server was never turned on.
        NeverEnabled,
        /// No socket, or nobody listening: server off or app closed.
        NotListening,
        /// The app refused the secret.
        Refused,
        /// Every connection slot is taken.
        Busy,
        Other(std::io::Error),
    }

    /// Connects and authenticates (the relay side, blocking).
    pub fn connect(dir: &Path) -> Result<std::os::unix::net::UnixStream, ClientError> {
        let secret = match read_secret(dir) {
            Ok(secret) => secret,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(ClientError::NeverEnabled)
            }
            Err(error) => return Err(ClientError::Other(error)),
        };
        let stream = std::os::unix::net::UnixStream::connect(socket_path(dir)).map_err(
            |error| match error.kind() {
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused => {
                    ClientError::NotListening
                }
                _ => ClientError::Other(error),
            },
        )?;
        stream
            .set_read_timeout(Some(HANDSHAKE_TIMEOUT))
            .map_err(ClientError::Other)?;
        // The app closes a peer it has no handshake slot for.
        let closed = |error: std::io::Error| match error.kind() {
            std::io::ErrorKind::BrokenPipe | std::io::ErrorKind::ConnectionReset => {
                ClientError::Busy
            }
            _ => ClientError::Other(error),
        };
        (&stream)
            .write_all(format!("{}\n", handshake(&secret)).as_bytes())
            .map_err(closed)?;
        let mut reply = String::new();
        BufReader::new(&stream)
            .read_line(&mut reply)
            .map_err(closed)?;
        if reply.is_empty() {
            return Err(ClientError::Busy);
        }
        let reply = serde_json::from_str::<Value>(&reply).unwrap_or(Value::Null);
        if reply.get("ok").and_then(Value::as_bool) != Some(true) {
            return Err(match reply.get("error").and_then(Value::as_str) {
                Some("busy") => ClientError::Busy,
                _ => ClientError::Refused,
            });
        }
        stream.set_read_timeout(None).map_err(ClientError::Other)?;
        Ok(stream)
    }
}
