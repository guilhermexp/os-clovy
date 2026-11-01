//! CDP transport restricted to inherited anonymous pipes. No discovery or listeners.
use crate::native::cdp::types::{CdpEvent, CdpMessage};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{BufRead, BufReader, Write},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
    sync::{Arc, Mutex as StdMutex},
};
use tokio::sync::{broadcast, mpsc, oneshot, Mutex};
const LIMIT: usize = 16 * 1024 * 1024;
fn valid_observation_breakpoint(id: &str, marker: &str) -> bool {
    let Some(nonce) = marker.strip_prefix("clovy-private-observation:") else {
        return false;
    };
    if nonce.len() != 32
        || !nonce
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return false;
    }
    id == format!("br:1:0:0:{marker}")
}
fn observation_event(method: &str, params: &Value, owned: &HashSet<String>) -> Value {
    if method == "Debugger.resumed" {
        return Value::Null;
    }
    let Some(hits) = params["hitBreakpoints"].as_array() else {
        return json!({"ownHits":[],"foreignPaused":true});
    };
    let mut own_hits = Vec::new();
    let mut foreign = hits.is_empty() || hits.len() > 16;
    for hit in hits.iter().take(16) {
        match hit.as_str() {
            Some(id) if owned.contains(id) => own_hits.push(id),
            _ => foreign = true,
        }
    }
    json!({"ownHits":own_hits,"foreignPaused":foreign})
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<CdpMessage>>>>;
type Taps = Arc<StdMutex<Vec<(&'static [&'static str], mpsc::UnboundedSender<CdpEvent>)>>>;
#[derive(Debug, Clone)]
pub struct RawCdpMessage {
    pub text: String,
    pub session_id: Option<String>,
}
pub struct CdpClient {
    writer: Arc<StdMutex<Option<File>>>,
    observation_breakpoints: Arc<StdMutex<HashSet<String>>>,
    redacted_nodes: StdMutex<HashSet<i64>>,
    redacted_values: StdMutex<Vec<String>>,
    pending: Pending,
    events: broadcast::Sender<CdpEvent>,
    closed: Arc<AtomicBool>,
    next_id: AtomicU64,
    taps: Taps,
    sessions: Arc<StdMutex<HashMap<String, mpsc::Sender<CdpEvent>>>>,
}
impl CdpClient {
    pub fn register_observation_breakpoint(&self, id: &str, marker: &str) -> Result<(), String> {
        if !valid_observation_breakpoint(id, marker) {
            return Err("redaction_failed".into());
        }
        let mut owned = self.observation_breakpoints.lock().unwrap();
        if owned.len() >= 4 {
            return Err("redaction_failed".into());
        }
        owned.insert(id.to_owned());
        Ok(())
    }
    pub fn unregister_observation_breakpoint(&self, id: &str) {
        self.observation_breakpoints.lock().unwrap().remove(id);
    }
    pub fn set_redacted_nodes(&self, nodes: HashSet<i64>, values: Vec<String>) {
        *self.redacted_values.lock().unwrap() = values;
        *self.redacted_nodes.lock().unwrap() = nodes;
    }
    pub fn from_pipe(read: File, write: File) -> Self {
        let observation_breakpoints = Arc::new(StdMutex::new(HashSet::new()));
        let reader_breakpoints = observation_breakpoints.clone();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let (events, _) = broadcast::channel(128);
        let closed = Arc::new(AtomicBool::new(false));
        let taps: Taps = Arc::new(StdMutex::new(Vec::new()));
        let sessions: Arc<StdMutex<HashMap<String, mpsc::Sender<CdpEvent>>>> =
            Arc::new(StdMutex::new(HashMap::new()));
        let (p, e, c, t, s) = (
            pending.clone(),
            events.clone(),
            closed.clone(),
            taps.clone(),
            sessions.clone(),
        );
        std::thread::spawn(move || {
            let mut reader = BufReader::new(read);
            loop {
                // take() bounds allocation before the delimiter, including a malicious renderer.
                let mut bytes = Vec::new();
                let count =
                    std::io::Read::take(&mut reader, (LIMIT + 1) as u64).read_until(0, &mut bytes);
                if !matches!(count, Ok(n) if n > 0)
                    || bytes.len() > LIMIT
                    || bytes.last() != Some(&0)
                {
                    break;
                }
                bytes.pop();
                let Ok(msg) = serde_json::from_slice::<CdpMessage>(&bytes) else {
                    break;
                };
                if let Some(id) = msg.id {
                    if let Some(tx) = p.blocking_lock().remove(&id) {
                        let _ = tx.send(msg);
                    }
                } else if let Some(method) = msg.method {
                    // No console, response-body, credential, plugin or streaming event buffer.
                    if !matches!(
                        method.as_str(),
                        "Page.domContentEventFired"
                            | "Page.loadEventFired"
                            | "Page.lifecycleEvent"
                            | "Debugger.paused"
                            | "Debugger.resumed"
                    ) {
                        continue;
                    }
                    let ev = CdpEvent {
                        method: method.clone(),
                        params: if method.starts_with("Debugger.") {
                            observation_event(
                                &method,
                                &msg.params.unwrap_or(Value::Null),
                                &reader_breakpoints.lock().unwrap(),
                            )
                        } else {
                            msg.params.unwrap_or(Value::Null)
                        },
                        session_id: msg.session_id,
                    };
                    if let Some(tx) = ev
                        .session_id
                        .as_ref()
                        .and_then(|id| s.lock().unwrap().get(id).cloned())
                    {
                        let _ = tx.try_send(ev.clone());
                    }
                    t.lock().unwrap().retain(|(methods, tx)| {
                        !methods.contains(&ev.method.as_str()) || tx.send(ev.clone()).is_ok()
                    });
                    let _ = e.send(ev);
                }
            }
            c.store(true, Ordering::SeqCst);
            p.blocking_lock().clear();
        });
        Self {
            writer: Arc::new(StdMutex::new(Some(write))),
            observation_breakpoints,
            redacted_nodes: StdMutex::new(HashSet::new()),
            redacted_values: StdMutex::new(Vec::new()),
            pending,
            events,
            closed,
            next_id: AtomicU64::new(1),
            taps,
            sessions,
        }
    }
    pub async fn connect(_: &str) -> Result<Self, String> {
        Err("Private host forbids CDP endpoints".into())
    }
    pub async fn connect_with_headers(
        _: &str,
        _: Option<Vec<(String, String)>>,
    ) -> Result<Self, String> {
        Self::connect("").await
    }
    pub async fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.writer.lock().unwrap().take();
        self.pending.lock().await.clear();
    }
    pub async fn send_command(
        &self,
        method: &str,
        params: Option<Value>,
        sid: Option<&str>,
    ) -> Result<Value, String> {
        if self.closed.load(Ordering::SeqCst) {
            return Err("CDP pipe closed".into());
        }
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let mut v = json!({"id":id,"method":method});
        if let Some(p) = params {
            v["params"] = p;
        }
        if let Some(s) = sid.filter(|s| !s.is_empty()) {
            v["sessionId"] = json!(s);
        }
        let mut bytes = serde_json::to_vec(&v).map_err(|_| "CDP encoding")?;
        if bytes.len() > LIMIT {
            return Err("CDP request too large".into());
        }
        bytes.push(0);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().await.insert(id, tx);
        let writer = self.writer.clone();
        let write = tokio::task::spawn_blocking(move || {
            let mut w = writer.lock().unwrap();
            let w = w.as_mut().ok_or("CDP pipe closed")?;
            w.write_all(&bytes).map_err(|_| "CDP pipe write failed")
        });
        let outcome = tokio::time::timeout(std::time::Duration::from_secs(15), async {
            write.await.map_err(|_| "CDP writer lost")??;
            let msg = rx.await.map_err(|_| "CDP pipe closed")?;
            if msg.error.is_some() {
                Err("CDP command rejected")
            } else {
                redact_response(
                    method,
                    msg.result.unwrap_or(Value::Null),
                    &self.redacted_nodes.lock().unwrap(),
                    &self.redacted_values.lock().unwrap(),
                )
            }
        })
        .await;
        self.pending.lock().await.remove(&id);
        outcome
            .map_err(|_| "CDP command timed out".to_string())?
            .map_err(String::from)
    }
    pub async fn send_command_typed<P: serde::Serialize, R: serde::de::DeserializeOwned>(
        &self,
        m: &str,
        p: &P,
        s: Option<&str>,
    ) -> Result<R, String> {
        let v = self
            .send_command(
                m,
                Some(serde_json::to_value(p).map_err(|_| "CDP encoding")?),
                s,
            )
            .await?;
        serde_json::from_value(v).map_err(|_| "Invalid CDP response".into())
    }
    pub async fn send_command_no_params(&self, m: &str, s: Option<&str>) -> Result<Value, String> {
        self.send_command(m, None, s).await
    }
    pub async fn send_command_no_wait(
        &self,
        m: &str,
        p: Option<Value>,
        s: Option<&str>,
    ) -> Result<(), String> {
        self.send_command(m, p, s).await.map(|_| ())
    }
    pub fn subscribe(&self) -> broadcast::Receiver<CdpEvent> {
        self.events.subscribe()
    }
    /// Count the actual in-flight command map for upstream cancellation tests.
    #[cfg(test)]
    pub(crate) async fn pending_len(&self) -> usize {
        self.pending.lock().await.len()
    }
    pub fn subscribe_methods(
        &self,
        methods: &'static [&'static str],
    ) -> mpsc::UnboundedReceiver<CdpEvent> {
        let (tx, rx) = mpsc::unbounded_channel();
        self.taps.lock().unwrap().push((methods, tx));
        rx
    }
    pub fn subscribe_session(&self, id: &str) -> mpsc::Receiver<CdpEvent> {
        let (tx, rx) = mpsc::channel(16);
        self.sessions.lock().unwrap().insert(id.into(), tx);
        rx
    }
    pub fn unsubscribe_session(&self, id: &str) {
        self.sessions.lock().unwrap().remove(id);
    }
    pub fn subscribe_raw(&self) -> broadcast::Receiver<RawCdpMessage> {
        let (_, rx) = broadcast::channel(1);
        rx
    }
    pub fn inspect_handle(&self) -> InspectProxyHandle {
        InspectProxyHandle
    }
    pub async fn send_raw(&self, _: String) -> Result<(), String> {
        Err("Private host forbids raw CDP".into())
    }
}
pub struct InspectProxyHandle;
impl InspectProxyHandle {
    pub async fn send_raw(&self, _: String) -> Result<(), String> {
        Err("Private host forbids inspect proxy".into())
    }
    pub fn subscribe_raw(&self) -> broadcast::Receiver<RawCdpMessage> {
        let (_, rx) = broadcast::channel(1);
        rx
    }
}

// Sanitize one complete AX response before any upstream snapshot caller sees it.
// Raw values and editable descendants come from this response, never only a prior DOM inventory.
fn redact_response(
    method: &str,
    mut v: Value,
    redacted: &HashSet<i64>,
    inventory: &[String],
) -> Result<Value, &'static str> {
    if !method.starts_with("Accessibility.")
        || matches!(method, "Accessibility.enable" | "Accessibility.disable")
    {
        return Ok(v);
    }
    let nodes = v
        .get_mut("nodes")
        .and_then(Value::as_array_mut)
        .ok_or("redaction_failed")?;
    let mut values = inventory.to_vec();
    let mut private_children = HashSet::<String>::new();
    for node in nodes.iter() {
        if let Some(value) = node["value"]["value"].as_str().filter(|v| !v.is_empty()) {
            values.push(value.to_owned());
            let trimmed = value.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
            if !trimmed.is_empty() && trimmed != value {
                values.push(trimmed.to_owned());
            }
            let normalized = value.split_whitespace().collect::<Vec<_>>().join(" ");
            if !normalized.is_empty() && normalized != value {
                values.push(normalized);
            }
        }
        let editable = node["properties"].as_array().is_some_and(|props| {
            props.iter().any(|p| {
                p["name"] == "editable"
                    && (p["value"]["value"].as_bool() == Some(true)
                        || matches!(p["value"]["value"].as_str(), Some("plaintext" | "richtext")))
            })
        });
        let field = matches!(
            node["role"]["value"].as_str(),
            Some("textbox" | "searchbox" | "combobox" | "spinbutton")
        );
        if editable || field {
            for child in node["childIds"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                private_children.insert(child.to_owned());
            }
        }
    }
    values.sort_by_key(|v| std::cmp::Reverse(v.len()));
    values.dedup();
    if values.len() > 4000 || values.iter().map(String::len).sum::<usize>() > 8 * 1024 * 1024 {
        return Err("redaction_failed");
    }
    loop {
        let count = private_children.len();
        for node in nodes.iter() {
            if node["nodeId"]
                .as_str()
                .is_some_and(|id| private_children.contains(id))
            {
                for child in node["childIds"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    private_children.insert(child.to_owned());
                }
            }
        }
        if count == private_children.len() {
            break;
        }
    }
    for node in nodes {
        let private = node["backendDOMNodeId"]
            .as_i64()
            .is_some_and(|id| redacted.contains(&id))
            || node["nodeId"]
                .as_str()
                .is_some_and(|id| private_children.contains(id));
        for key in ["name", "description"] {
            if let Some(text) = node[key]["value"].as_str() {
                let mut safe = if private {
                    "[redacted]".to_owned()
                } else {
                    text.to_owned()
                };
                for value in &values {
                    safe = safe.replace(value, "[redacted]");
                }
                // Drop AX computed-name sources too: they can retain raw field content.
                node[key] = json!({"type":"computedString", "value":safe});
            } else if let Some(object) = node.as_object_mut() {
                object.remove(key);
            }
        }
        if let Some(object) = node.as_object_mut() {
            object.remove("value");
        }
    }
    Ok(v)
}

#[cfg(test)]
mod private_redaction_tests {
    use super::*;
    #[test]
    fn debugger_notifications_never_retain_foreign_urls_or_stack_values() {
        let marker = "clovy-private-observation:aaaaaaaaaaaa4aaa8aaaaaaaaaaaaaaa";
        let own = format!("br:1:0:0:{marker}");
        assert!(valid_observation_breakpoint(&own, marker));
        let foreign = "br:1:0:0:https://private.invalid/?otp=123456";
        assert!(!valid_observation_breakpoint(foreign, marker));
        let raw = json!({"hitBreakpoints":[own,foreign],"callFrames":[{"url":"secret-url","scopeChain":[{"value":"secret-local"}]}]});
        let safe = observation_event("Debugger.paused", &raw, &HashSet::from([own.clone()]));
        assert_eq!(safe["ownHits"], json!([own]));
        assert_eq!(safe["foreignPaused"], true);
        let encoded = safe.to_string();
        assert!(
            !encoded.contains("private.invalid")
                && !encoded.contains("123456")
                && !encoded.contains("secret-url")
                && !encoded.contains("secret-local")
        );
        assert_eq!(
            observation_event(
                "Debugger.paused",
                &json!({"hitBreakpoints":[]}),
                &HashSet::new()
            )["foreignPaused"],
            true
        );
    }

    #[test]
    fn ax_current_and_new_field_values_redact_names_without_stale_inventory() {
        let raw = json!({"nodes":[
            {"nodeId":"1","role":{"value":"textbox"},"name":{"value":"private-B","sources":[{"value":"private-B"}]},"value":{"value":"private-B"}},
            {"nodeId":"2","role":{"value":"textbox"},"name":{"value":"Fresh field"},"value":{"value":"new-C"},"childIds":["3"]},
            {"nodeId":"3","role":{"value":"StaticText"},"name":{"value":"new-C"}},
            {"nodeId":"4","role":{"value":"button"},"name":{"value":"Query"}},
            {"nodeId":"5","role":{"value":"button"},"name":{"value":"Copied new-C"}}
        ]});
        let safe = redact_response(
            "Accessibility.getFullAXTree",
            raw,
            &HashSet::new(),
            &["old-A".into()],
        )
        .unwrap();
        let encoded = safe.to_string();
        assert!(!encoded.contains("private-B") && !encoded.contains("new-C"));
        assert_eq!(safe["nodes"][1]["name"]["value"], "Fresh field");
        assert_eq!(safe["nodes"][3]["name"]["value"], "Query");
        assert_eq!(safe["nodes"][4]["name"]["value"], "Copied [redacted]");
    }
}
