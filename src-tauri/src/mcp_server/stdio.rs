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

#[cfg(not(unix))]
use std::io::{BufRead, Write};
use std::path::PathBuf;

use serde_json::{json, Value};

use super::protocol;

pub const OFF_MESSAGE: &str = "The Clovy MCP server is off or Clovy is not open. Open Clovy and turn on the MCP server in Settings, Agent.";
pub const NEVER_ENABLED_MESSAGE: &str = "The Clovy MCP server has not been turned on yet. Open Clovy and turn it on in Settings, Agent.";
pub const REFUSED_MESSAGE: &str = "Clovy refused the connection because the installation secret did not match. Copy the configuration again from Clovy Settings, Agent.";
pub const DROPPED_MESSAGE: &str =
    "Clovy closed the connection before answering (the MCP server was turned off or Clovy quit).";

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
    let id = protocol::request_id(message)?;
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
                for message in orphans {
                    if let Some(response) = fallback(&message, DROPPED_MESSAGE) {
                        emit(&thread_output, &response);
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
            ClientError::Other(error) => format!("Could not reach Clovy: {error}"),
        }
    }

    /// Relays `input` lines until end of input.
    pub fn relay(dir: &Path, input: impl BufRead, output: Box<dyn Write + Send>) {
        let output: Output = Arc::new(Mutex::new(output));
        let mut connection: Option<Connection> = None;
        for line in input.lines() {
            let Ok(line) = line else { break };
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let message = match serde_json::from_str::<Value>(line) {
                Ok(message) => message,
                Err(error) => {
                    emit(
                        &output,
                        &protocol::error_response(
                            Value::Null,
                            protocol::PARSE_ERROR,
                            error.to_string(),
                        ),
                    );
                    continue;
                }
            };
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
