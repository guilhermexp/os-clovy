use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader as AsyncBufReader};
use tokio::sync::{mpsc, watch, OwnedSemaphorePermit, Semaphore};

use super::*;
use crate::mcp_server::turns::{BoundTurn, TurnBinding, TurnRegistry, TURN_EXPIRED};

pub(super) const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a revoked connection waits for the answers of the calls it
/// was serving before it closes.
const REVOKE_GRACE: Duration = Duration::from_secs(2);
pub(super) const ACCEPT_ERROR_BACKOFF: Duration = Duration::from_millis(100);

/// Answers one MCP message (`None` for notifications), whatever the
/// connection's turn.
pub type Handler =
    Arc<dyn Fn(Value) -> Pin<Box<dyn Future<Output = Option<Value>> + Send>> + Send + Sync>;

/// Answers one MCP message of a connection bound to a turn, or not
/// (`None`).
pub type TurnHandler = Arc<
    dyn Fn(Value, Option<Arc<TurnBinding>>) -> Pin<Box<dyn Future<Output = Option<Value>> + Send>>
        + Send
        + Sync,
>;

/// What a listener answers with: either handler kind.
pub struct Service(pub(super) TurnHandler);

impl From<TurnHandler> for Service {
    fn from(handler: TurnHandler) -> Self {
        Self(handler)
    }
}

impl From<Handler> for Service {
    fn from(handler: Handler) -> Self {
        Self(Arc::new(move |message, _turn| handler(message)))
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

pub(super) struct Authentication {
    pub same_user: bool,
    pub secret: Arc<str>,
}

pub(super) async fn serve<S>(
    stream: S,
    authentication: Authentication,
    handshake: OwnedSemaphorePermit,
    slots: Arc<Semaphore>,
    turns: TurnRegistry,
    handler: TurnHandler,
    mut shutdown: watch::Receiver<bool>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let (read, mut write) = tokio::io::split(stream);
    let mut reader = AsyncBufReader::new(read);
    if *shutdown.borrow() {
        return;
    }
    let hello = tokio::select! {
        biased;
        _ = shutdown.changed() => return,
        hello = tokio::time::timeout(HANDSHAKE_TIMEOUT, read_line(&mut reader)) => hello,
    };
    let offered = match hello {
        Ok(Ok(Some(line))) => serde_json::from_slice::<Value>(&line).ok(),
        _ => None,
    };
    let accepted = authentication.same_user
        && offered.as_ref().is_some_and(|hello| {
            hello.get("clovyMcp").and_then(Value::as_u64) == Some(HANDSHAKE_VERSION)
                && hello
                    .get("secret")
                    .and_then(Value::as_str)
                    .is_some_and(|offered| secrets_match(&authentication.secret, offered))
        });
    // The turn is looked up only after the secret matched: a token alone
    // learns nothing.
    let turn = match accepted.then(|| offered.as_ref().and_then(|hello| hello.get("turn"))) {
        None | Some(None) | Some(Some(Value::Null)) => Ok(None),
        Some(Some(Value::String(token))) => turns.bind(token).map(Some).ok_or(()),
        Some(Some(_)) => Err(()),
    };
    // Held until this connection ends.
    let slot = (accepted && turn.is_ok())
        .then(|| slots.try_acquire_owned().ok())
        .flatten();
    drop(handshake);
    let reply = match (accepted, &turn, &slot) {
        (false, _, _) => {
            json!({ "clovyMcp": HANDSHAKE_VERSION, "ok": false, "error": "unauthorized" })
        }
        (true, Err(()), _) => {
            json!({ "clovyMcp": HANDSHAKE_VERSION, "ok": false, "error": TURN_EXPIRED })
        }
        (true, Ok(_), Some(_)) => json!({ "clovyMcp": HANDSHAKE_VERSION, "ok": true }),
        (true, Ok(_), None) => {
            json!({ "clovyMcp": HANDSHAKE_VERSION, "ok": false, "error": "busy" })
        }
    };
    let mut turn: Option<BoundTurn> = turn.ok().flatten();
    let binding = turn.as_ref().map(|turn| Arc::clone(&turn.binding));
    let reply_line = format!("{reply}\n");
    let replied = tokio::select! {
        biased;
        _ = shutdown.changed() => false,
        result = tokio::time::timeout(HANDSHAKE_TIMEOUT, write.write_all(reply_line.as_bytes())) => matches!(result, Ok(Ok(()))),
    };
    if !replied || slot.is_none() {
        if replied {
            tokio::select! {
                _ = shutdown.changed() => {},
                _ = tokio::time::timeout(HANDSHAKE_TIMEOUT, write.shutdown()) => {},
            }
        }
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
    let mut calls = tokio::task::JoinSet::new();
    let (cancel_calls, call_cancelled) = watch::channel(false);
    let mut cut = false;
    loop {
        while calls.try_join_next().is_some() {}
        // Backpressure: the next line is read only once a request slot
        // is free, so a fast client waits in its own socket buffer.
        let permit = tokio::select! {
            biased;
            _ = shutdown.changed() => break,
            _ = revoked(&mut turn) => { cut = true; break; }
            permit = Arc::clone(&in_flight).acquire_owned() => match permit {
                Ok(permit) => permit,
                Err(_) => break,
            },
        };
        tokio::select! {
            biased;
            _ = shutdown.changed() => break,
            _ = revoked(&mut turn) => { cut = true; break; }
            line = read_line(&mut reader) => {
                let Ok(Some(line)) = line else { break };
                if line.iter().all(u8::is_ascii_whitespace) {
                    continue;
                }
                let responses = responses.clone();
                let handler = Arc::clone(&handler);
                let binding = binding.clone();
                let mut call_cancelled = call_cancelled.clone();
                calls.spawn(async move {
                    let response = match serde_json::from_slice::<Value>(&line) {
                        Ok(message) => {
                            let id = crate::mcp_server::protocol::request_id(&message);
                            let is_tool = crate::mcp_server::protocol::method(&message) == Some("tools/call");
                            let operation = handler(message, binding);
                            tokio::pin!(operation);
                            tokio::select! {
                                biased;
                                response = &mut operation => response,
                                _ = call_cancelled.changed() => {
                                    match tokio::time::timeout(REVOKE_GRACE / 2, &mut operation).await {
                                        Ok(response) => response,
                                        Err(_) => id.map(|id| if is_tool {
                                            let mut error = crate::domain::types::AppError::new("run_cancelled", "The run was cancelled after the call was admitted. Inspect its target before retrying.");
                                            error.details = Some(serde_json::json!({"delivery":"unknown","effect":"unknown","retrySafe":false,"callId":id.clone()}));
                                            crate::mcp_server::protocol::result_response(id, crate::mcp_server::protocol::tool_error_for(&error))
                                        } else {
                                            crate::mcp_server::protocol::error_response(id, crate::mcp_server::protocol::SERVER_UNAVAILABLE, "The run was cancelled. (run_cancelled)")
                                        }),
                                    }
                                },
                            }
                        },
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
        }
    }
    if *shutdown.borrow() {
        // App shutdown: pending answers are dropped.
        calls.abort_all();
        writer.abort();
        let _ = writer.await;
    } else if cut {
        // Stop in-flight handlers as well as rejecting new requests.
        // A remote dispatch already sent may still have an uncertain result.
        let _ = cancel_calls.send(true);
        drop(responses);
        let mut writer = writer;
        if tokio::time::timeout(REVOKE_GRACE, &mut writer)
            .await
            .is_err()
        {
            writer.abort();
            let _ = writer.await;
        }
    } else {
        // EOF still allows receipts, but a non-reading peer cannot keep an
        // unbounded writer/handler alive after closing its input.
        drop(responses);
        let mut writer = writer;
        if tokio::time::timeout(REVOKE_GRACE, &mut writer)
            .await
            .is_err()
        {
            writer.abort();
            let _ = writer.await;
        }
    }
    calls.abort_all();
    while calls.join_next().await.is_some() {}
    drop(slot);
}

/// Resolves when the connection's turn is revoked; never for an unbound
/// connection.
async fn revoked(turn: &mut Option<BoundTurn>) {
    match turn {
        Some(turn) => turn.revoked().await,
        None => std::future::pending().await,
    }
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
    /// The turn token is unknown or revoked: the CLI turn has ended.
    TurnExpired,
    Other(std::io::Error),
}
