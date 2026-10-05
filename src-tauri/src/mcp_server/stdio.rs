//! `clovy-mcp`: the stdio MCP server an MCP client spawns. It relays each
//! JSON-RPC line to the running app over the authenticated channel
//! (`super::channel`) and copies the app's answers to stdout. While the app
//! cannot be reached (closed, server turned off, wrong secret) it answers by
//! itself: `initialize` and `ping` succeed so the client can show the reason,
//! tool calls return an error result saying why, and other requests fail with
//! the same message.
//!
//! Usage: `clovy-mcp --dir "<app data dir>/mcp"` (the configuration snippets
//! in Settings, Agent carry the right directory).

use std::io::BufRead;
#[cfg(not(unix))]
use std::io::Write;
use std::path::PathBuf;

use serde_json::{json, Value};

use super::protocol;

pub const OFF_MESSAGE: &str = "The Clovy MCP server is off or Clovy is not open. Open Clovy and turn on the MCP server in Settings, Agent.";
pub const NEVER_ENABLED_MESSAGE: &str = "The Clovy MCP server has not been turned on yet. Open Clovy and turn it on in Settings, Agent.";
pub const REFUSED_MESSAGE: &str = "Clovy refused the connection because the installation secret did not match. Copy the configuration again from Clovy Settings, Agent.";
pub const BUSY_MESSAGE: &str = "Clovy is already serving as many MCP connections as it allows. Close another MCP client, then try again.";
pub const DROPPED_MESSAGE: &str =
    "Clovy closed the connection before answering (the MCP server was turned off or Clovy quit).";
pub const PRODUCTION_DATA_MESSAGE: &str = "This development build of clovy-mcp does not connect to the installed Clovy's data. Copy the configuration from the development app, or set OS_CLOVY_USE_PROD_DATA_DIR=1 to allow it.";
pub const TOO_LONG_MESSAGE: &str = "The message is longer than 4 MiB.";

/// The `--dir` argument.
pub fn parse_dir(args: &[String]) -> Result<PathBuf, String> {
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--dir=") {
            return Ok(PathBuf::from(value));
        }
        if arg == "--dir" {
            return args
                .next()
                .map(PathBuf::from)
                .ok_or_else(|| "--dir needs a directory".to_string());
        }
    }
    Err("usage: clovy-mcp --dir <Clovy data directory>/mcp (copy the configuration from Clovy Settings, Agent)".to_string())
}

/// The local answer to `message` while the app is unreachable for `reason`.
pub fn fallback(message: &Value, reason: &str) -> Option<Value> {
    let id = match protocol::classify(message) {
        protocol::Incoming::Request { id, .. } => id,
        protocol::Incoming::Invalid { id, reason } => {
            return Some(protocol::error_response(
                id,
                protocol::INVALID_REQUEST,
                reason,
            ))
        }
        protocol::Incoming::Notification | protocol::Incoming::Response => return None,
    };
    let params = message.get("params").cloned().unwrap_or_else(|| json!({}));
    Some(match protocol::method(message).unwrap_or_default() {
        "initialize" => protocol::result_response(
            id,
            protocol::initialize_result(&params, &format!("Clovy is not reachable: {reason}")),
        ),
        "ping" => protocol::result_response(id, json!({})),
        "tools/call" => protocol::result_response(id, protocol::tool_error(reason)),
        _ => protocol::error_response(id, protocol::SERVER_UNAVAILABLE, reason),
    })
}

/// Debug builds keep away from the installed app's data, like the debug app
/// itself (`crate::app_paths`): a channel directory under the production
/// data directory is refused unless `OS_CLOVY_USE_PROD_DATA_DIR` is set.
pub fn production_data_refusal(dir: &std::path::Path) -> Option<&'static str> {
    if !cfg!(debug_assertions) || crate::app_paths::use_prod_data_dir() {
        return None;
    }
    let production = crate::extension_host::production_app_data_dir()?;
    let resolve =
        |path: &std::path::Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    resolve(dir)
        .starts_with(resolve(&production))
        .then_some(PRODUCTION_DATA_MESSAGE)
}

/// One input line.
pub enum InputLine {
    Line(Vec<u8>),
    /// Longer than the limit: skipped to its end without being kept.
    TooLong,
}

/// Reads one line of at most `max` bytes (newline excluded). A longer line
/// is consumed to its newline while keeping nothing, so a client cannot make
/// the relay buffer an unbounded line.
pub fn read_bounded_line(
    input: &mut impl BufRead,
    max: usize,
) -> std::io::Result<Option<InputLine>> {
    let mut line = Vec::new();
    let mut too_long = false;
    let mut read_any = false;
    loop {
        let available = match input.fill_buf() {
            Ok(available) => available,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if available.is_empty() {
            break;
        }
        read_any = true;
        let newline = available.iter().position(|byte| *byte == b'\n');
        let content = newline.unwrap_or(available.len());
        if !too_long {
            if line.len() + content > max {
                too_long = true;
                line = Vec::new();
            } else {
                line.extend_from_slice(&available[..content]);
            }
        }
        input.consume(newline.map_or(content, |index| index + 1));
        if newline.is_some() {
            break;
        }
    }
    let read = if too_long {
        InputLine::TooLong
    } else {
        InputLine::Line(line)
    };
    Ok(read_any.then_some(read))
}

/// Entry point for the `clovy-mcp` binary.
pub fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let dir = match parse_dir(&args) {
        Ok(dir) => dir,
        Err(message) => {
            eprintln!("clovy-mcp: {message}");
            return 2;
        }
    };
    relay(&dir, std::io::stdin().lock(), Box::new(std::io::stdout()));
    0
}

#[cfg(not(unix))]
pub fn relay(_dir: &std::path::Path, input: impl BufRead, output: Box<dyn Write + Send>) {
    let mut output = output;
    for line in input.lines().map_while(Result::ok) {
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(response) =
            fallback(&message, "The Clovy MCP server is available on macOS only.")
        {
            let _ = writeln!(output, "{response}");
            let _ = output.flush();
        }
    }
}

#[cfg(unix)]
pub use unix::relay;

#[cfg(unix)]
mod unix {
    use std::collections::HashMap;
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::UnixStream;
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, PoisonError};

    use serde_json::Value;

    use super::*;
    use crate::mcp_server::channel::{self, ClientError};

    type Output = Arc<Mutex<Box<dyn Write + Send>>>;

    fn emit(output: &Output, message: &Value) {
        let mut output = output.lock().unwrap_or_else(PoisonError::into_inner);
        let _ = writeln!(output, "{message}");
        let _ = output.flush();
    }

    /// Why requests lost their connection: the listener removes its socket
    /// before closing connections, so a missing socket means the server was
    /// turned off; otherwise Clovy dropped the connection (or quit). Checks the
    /// file only: connecting would take a connection slot from real clients.
    fn dropped_reason(dir: &Path) -> &'static str {
        if channel::socket_path(dir).exists() {
            DROPPED_MESSAGE
        } else {
            OFF_MESSAGE
        }
    }

    /// An authenticated connection and the requests still waiting on it.
    struct Connection {
        stream: UnixStream,
        alive: Arc<AtomicBool>,
        pending: Arc<Mutex<HashMap<String, Value>>>,
    }

    impl Connection {
        fn open(dir: &Path, output: &Output) -> Result<Self, ClientError> {
            let stream = channel::connect(dir)?;
            let reader = stream.try_clone().map_err(ClientError::Other)?;
            let alive = Arc::new(AtomicBool::new(true));
            let pending: Arc<Mutex<HashMap<String, Value>>> = Arc::default();
            let (thread_alive, thread_pending, thread_output) =
                (Arc::clone(&alive), Arc::clone(&pending), Arc::clone(output));
            let probe_dir = dir.to_path_buf();
            std::thread::spawn(move || {
                for line in BufReader::new(reader).lines() {
                    let Ok(line) = line else { break };
                    let Ok(message) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    if let Some(id) = message.get("id") {
                        thread_pending
                            .lock()
                            .unwrap_or_else(PoisonError::into_inner)
                            .remove(&id.to_string());
                    }
                    emit(&thread_output, &message);
                }
                thread_alive.store(false, Ordering::SeqCst);
                let orphans: Vec<Value> = thread_pending
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .drain()
                    .map(|(_, message)| message)
                    .collect();
                if !orphans.is_empty() {
                    let reason = dropped_reason(&probe_dir);
                    for message in orphans {
                        if let Some(response) = fallback(&message, reason) {
                            emit(&thread_output, &response);
                        }
                    }
                }
            });
            Ok(Self {
                stream,
                alive,
                pending,
            })
        }

        fn send(&mut self, line: &str, message: &Value) -> bool {
            let key = protocol::request_id(message).map(|id| id.to_string());
            if let Some(key) = &key {
                self.pending
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(key.clone(), message.clone());
            }
            let sent = self
                .stream
                .write_all(line.as_bytes())
                .and_then(|()| self.stream.write_all(b"\n"))
                .is_ok();
            if !sent {
                self.alive.store(false, Ordering::SeqCst);
                if let Some(key) = key {
                    self.pending
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .remove(&key);
                }
            }
            sent
        }
    }

    fn reason(error: &ClientError) -> String {
        match error {
            ClientError::NeverEnabled => NEVER_ENABLED_MESSAGE.to_string(),
            ClientError::NotListening => OFF_MESSAGE.to_string(),
            ClientError::Refused => REFUSED_MESSAGE.to_string(),
            ClientError::Busy => BUSY_MESSAGE.to_string(),
            ClientError::Other(error) => format!("Could not reach Clovy: {error}"),
        }
    }

    /// Relays `input` lines until end of input.
    pub fn relay(dir: &Path, mut input: impl BufRead, output: Box<dyn Write + Send>) {
        let output: Output = Arc::new(Mutex::new(output));
        let refused = production_data_refusal(dir);
        let mut connection: Option<Connection> = None;
        loop {
            let bytes = match read_bounded_line(&mut input, channel::MAX_LINE_BYTES) {
                Ok(Some(InputLine::Line(bytes))) => bytes,
                Ok(Some(InputLine::TooLong)) => {
                    emit(
                        &output,
                        &protocol::error_response(
                            Value::Null,
                            protocol::INVALID_REQUEST,
                            TOO_LONG_MESSAGE,
                        ),
                    );
                    continue;
                }
                Ok(None) | Err(_) => break,
            };
            let parsed = String::from_utf8(bytes)
                .map_err(|error| error.to_string())
                .and_then(|text| {
                    let line = text.trim().to_string();
                    if line.is_empty() {
                        return Ok(None);
                    }
                    serde_json::from_str::<Value>(&line)
                        .map(|message| Some((line, message)))
                        .map_err(|error| error.to_string())
                });
            let (line, message) = match parsed {
                Ok(Some(parsed)) => parsed,
                Ok(None) => continue,
                Err(error) => {
                    emit(
                        &output,
                        &protocol::error_response(Value::Null, protocol::PARSE_ERROR, error),
                    );
                    continue;
                }
            };
            // Invalid requests are answered here; they never reach the app.
            if let protocol::Incoming::Invalid { id, reason } = protocol::classify(&message) {
                emit(
                    &output,
                    &protocol::error_response(id, protocol::INVALID_REQUEST, reason),
                );
                continue;
            }
            if let Some(reason) = refused {
                if let Some(response) = fallback(&message, reason) {
                    emit(&output, &response);
                }
                continue;
            }
            let line = line.as_str();
            if connection
                .as_ref()
                .is_some_and(|connection| !connection.alive.load(Ordering::SeqCst))
            {
                connection = None;
            }
            let mut failure = None;
            for _attempt in 0..2 {
                if connection.is_none() {
                    match Connection::open(dir, &output) {
                        Ok(opened) => connection = Some(opened),
                        Err(error) => {
                            failure = Some(reason(&error));
                            break;
                        }
                    }
                }
                if connection
                    .as_mut()
                    .is_some_and(|connection| connection.send(line, &message))
                {
                    failure = None;
                    break;
                }
                connection = None;
                failure = Some(DROPPED_MESSAGE.to_string());
            }
            if let Some(reason) = failure {
                if let Some(response) = fallback(&message, &reason) {
                    emit(&output, &response);
                }
            }
        }
    }
}
