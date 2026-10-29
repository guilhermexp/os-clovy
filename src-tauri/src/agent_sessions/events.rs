//! Durable CLI chat events and compatibility hydration for saved sessions.
use super::{
    protocol::{RpcFrame, PROTOCOL_VERSION},
    AgentItemPayload, AgentRepository, AgentRunDto, TextPayload, ToolPayload,
};
use crate::domain::types::AppError;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;
pub const AGENT_RUNTIME_EVENT: &str = "clovy://agent-runtime-event";
pub(crate) fn emit_persisted_run_cancelled(
    app: &AppHandle,
    run: &AgentRunDto,
) -> Result<(), AppError> {
    app.emit(
        AGENT_RUNTIME_EVENT,
        json!({
            "protocolVersion": PROTOCOL_VERSION,
            "sessionId": run.session_id,
            "runId": run.id,
            "sequence": run.last_sequence.saturating_add(1),
            "eventId": Uuid::new_v4(),
            "method": "run.cancelled",
            "data": {
                "completedAt": run.completed_at,
            },
        }),
    )
    .map_err(|error| AppError::new("agent_event_emit_failed", error.to_string()))
}
/// An event persisted by [`persist_runtime_event`], ready to be emitted.
pub(crate) struct PersistedRuntimeEvent {
    pub method: String,
    pub event_id: String,
    pub data: Value,
}

/// Persists a CLI chat event before publishing it to the webview.
pub(crate) async fn persist_and_emit_event(
    app: &AppHandle,
    repository: &AgentRepository,
    frame: &RpcFrame,
) -> Result<(), AppError> {
    let Some(PersistedRuntimeEvent {
        method,
        event_id,
        data,
    }) = persist_runtime_event(repository, frame).await?
    else {
        return Ok(());
    };
    app.emit(AGENT_RUNTIME_EVENT, json!({ "protocolVersion": PROTOCOL_VERSION, "sessionId": frame.session_id, "runId": frame.run_id, "sequence": frame.sequence, "eventId": event_id, "method": method, "data": data })).map_err(|error| AppError::new("agent_event_emit_failed", error.to_string()))?;
    Ok(())
}

/// Applies one runtime event to the repository (items, run status, routine
/// projections) and returns what to emit, or `None` for an event that must
/// not be emitted (a late `run.started` for a terminal run).
pub(crate) async fn persist_runtime_event(
    repository: &AgentRepository,
    frame: &RpcFrame,
) -> Result<Option<PersistedRuntimeEvent>, AppError> {
    let method = frame.method.as_deref().unwrap_or_default();
    let params = frame.params.clone().unwrap_or_else(|| json!({}));
    let event_id = frame
        .event_id
        .clone()
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let created_at = crate::db::repositories::timestamp();
    let assistant_id = format!("assistant:{}", frame.run_id);
    let reasoning_id = format!("reasoning:{}", frame.run_id);
    let mut persistence_external_id = event_id.clone();
    let mut data = params.clone();
    let payload = match method {
        "message.delta" => {
            data["itemId"] = json!(assistant_id);
            data["role"] = json!("assistant");
            data["createdAt"] = json!(created_at);
            repository
                .append_assistant_message_delta(
                    &frame.session_id,
                    &frame.run_id,
                    frame.sequence,
                    params
                        .get("delta")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    &assistant_id,
                )
                .await?;
            None
        }
        "message.completed" => {
            let text = params
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or_default();
            data["itemId"] = json!(assistant_id);
            data["role"] = json!("assistant");
            data["createdAt"] = json!(created_at);
            repository
                .complete_assistant_message(
                    &frame.session_id,
                    &frame.run_id,
                    frame.sequence,
                    text,
                    &assistant_id,
                )
                .await?;
            None
        }
        "reasoning.delta" => {
            data["itemId"] = json!(reasoning_id);
            data["createdAt"] = json!(created_at);
            repository
                .append_reasoning_delta(
                    &frame.session_id,
                    &frame.run_id,
                    frame.sequence,
                    params
                        .get("delta")
                        .and_then(Value::as_str)
                        .unwrap_or_default(),
                    &reasoning_id,
                )
                .await?;
            None
        }
        "steering.consumed" => {
            persistence_external_id = steering_stable_id(&params, &event_id);
            data["itemId"] = json!(persistence_external_id.clone());
            data["createdAt"] = json!(created_at);
            Some(AgentItemPayload::Steering(TextPayload {
                text: params
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .into(),
                metadata: None,
            }))
        }
        "tool.started" => {
            data["itemId"] = json!(format!("tool-call:{event_id}"));
            data["createdAt"] = json!(created_at);
            Some(AgentItemPayload::ToolCall(tool_payload(&params, "running")))
        }
        "tool.completed" => {
            data["itemId"] = json!(format!("tool-result:{event_id}"));
            data["createdAt"] = json!(created_at);
            Some(AgentItemPayload::ToolResult(tool_payload(
                &params, "complete",
            )))
        }
        "tool.failed" => {
            data["itemId"] = json!(format!("tool-result:{event_id}"));
            data["createdAt"] = json!(created_at);
            Some(AgentItemPayload::ToolResult(tool_payload(
                &params, "failed",
            )))
        }
        "interruption.requested" => {
            let serialized = params
                .get("serializedState")
                .cloned()
                .unwrap_or(Value::Null);
            let usage = params.get("usage");
            repository
                .update_run_status(
                    &frame.run_id,
                    "waiting_for_user",
                    usage,
                    Some(&serialized),
                    None,
                )
                .await?;
            crate::routines::mark_agent_run_waiting(&repository.pool, &frame.run_id).await?;
            let kind = params
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("approval");
            let interruption_id = interruption_stable_id(&params, &event_id);
            persistence_external_id = interruption_external_id(&frame.run_id, &interruption_id);
            let interruption = match kind {
                "clarification" => {
                    json!({ "id": interruption_id, "sessionId": frame.session_id, "runId": frame.run_id, "status": "pending", "createdAt": created_at, "kind": "clarification", "question": params.get("question").cloned().unwrap_or_else(|| json!("What would you like Clovy to do?")), "choices": params.get("choices").cloned().unwrap_or_else(|| json!([])) })
                }
                "secret" => {
                    json!({ "id": interruption_id, "sessionId": frame.session_id, "runId": frame.run_id, "status": "pending", "createdAt": created_at, "kind": "secret", "reason": params.get("reason").cloned().unwrap_or_else(|| json!("Clovy needs a secret before it can continue.")) })
                }
                _ => {
                    let tool_name = params
                        .get("toolName")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown_tool");
                    if let Some(presentation) = params.get("approvalPresentation") {
                        json!({ "id": interruption_id, "toolCallId": params.get("callId").cloned().unwrap_or(Value::Null), "sessionId": frame.session_id, "runId": frame.run_id, "status": "pending", "createdAt": created_at, "kind": "approval", "toolName": tool_name, "title": presentation.get("title").cloned().unwrap_or_else(|| json!("Approval required")), "description": presentation.get("description").cloned().unwrap_or_else(|| json!("Review this Notion action.")), "command": presentation.get("command").cloned().unwrap_or_else(|| json!(tool_name)), "preview": presentation.get("preview").cloned().unwrap_or(Value::Null), "approvalBinding": params.get("approvalBinding").cloned().unwrap_or(Value::Null), "allowAlways": false })
                    } else {
                        let (operation_name, operation_description) =
                            approval_operation_identity(&params, tool_name);
                        let command = approval_command(&operation_name, params.get("arguments"));
                        json!({ "id": interruption_id, "toolCallId": params.get("callId").cloned().unwrap_or(Value::Null), "sessionId": frame.session_id, "runId": frame.run_id, "status": "pending", "createdAt": created_at, "kind": "approval", "toolName": tool_name, "title": "Approval required", "description": format!("Clovy wants to run {operation_description}. Review the requested operation before approving."), "command": command, "allowAlways": false })
                    }
                }
            };
            data = json!({ "itemId": persistence_external_id, "interruption": interruption });
            Some(AgentItemPayload::Interruption(data["interruption"].clone()))
        }
        "usage.updated" => {
            let current = repository.get_run(&frame.run_id).await?;
            repository
                .update_run_status(&frame.run_id, &current.status, Some(&params), None, None)
                .await?;
            None
        }
        "run.started" => {
            data["startedAt"] = json!(created_at);
            let run = repository
                .update_run_status(&frame.run_id, "running", None, None, None)
                .await?;
            if run.status != "running" {
                tracing::warn!(
                    run_id = %frame.run_id,
                    status = %run.status,
                    "ignored a late run.started event for a terminal run"
                );
                return Ok(None);
            }
            crate::routines::mark_agent_run_resumed(&repository.pool, &frame.run_id).await?;
            if params
                .get("compacted")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                let summary_text = params
                    .get("contextSummary")
                    .and_then(|summary| summary.get("text"))
                    .and_then(Value::as_str);
                let summary_metadata = params
                    .get("contextSummary")
                    .and_then(|summary| summary.get("metadata"));
                let removed_item_ids = params
                    .get("removedItemIds")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                if let Some(summary_text) = summary_text {
                    if let Some(summary) = repository
                        .replace_items_with_context_summary(
                            &frame.session_id,
                            &frame.run_id,
                            summary_text,
                            summary_metadata,
                            &removed_item_ids,
                        )
                        .await?
                    {
                        data["removedItemIds"] = json!(removed_item_ids);
                        data["contextSummary"] = json!({
                            "id": summary.id,
                            "sessionId": summary.session_id,
                            "runId": summary.run_id,
                            "sequence": summary.sequence,
                            "createdAt": summary.created_at,
                            "kind": "context_summary",
                            "text": summary_text,
                            "metadata": summary_metadata,
                        });
                    }
                }
            }
            None
        }
        "run.completed" => {
            data["completedAt"] = json!(created_at);
            repository
                .update_run_status(&frame.run_id, "completed", None, None, None)
                .await?;
            if let Err(error) =
                crate::routines::mark_agent_run_terminal(&repository.pool, &frame.run_id).await
            {
                tracing::warn!(agent_run_id = %frame.run_id, error_code = %error.code, "routine terminal projection failed");
            }
            None
        }
        "run.cancelled" => {
            data["completedAt"] = json!(created_at);
            repository
                .update_run_status(&frame.run_id, "cancelled", None, None, None)
                .await?;
            if let Err(error) =
                crate::routines::mark_agent_run_terminal(&repository.pool, &frame.run_id).await
            {
                tracing::warn!(agent_run_id = %frame.run_id, error_code = %error.code, "routine terminal projection failed");
            }
            None
        }
        "run.failed" => {
            let message = params
                .get("error")
                .and_then(Value::as_str)
                .map(sanitize_log)
                .unwrap_or_else(|| "Agent run failed.".into());
            let category = params
                .get("category")
                .and_then(Value::as_str)
                .filter(|value| matches!(*value, "tool" | "provider" | "runtime" | "context"))
                .unwrap_or("runtime");
            let code = params
                .get("code")
                .and_then(Value::as_str)
                .filter(|value| value.starts_with("agent_") || *value == "runtime_crashed")
                .unwrap_or("agent_runtime_failed");
            let retryable = params
                .get("retryable")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            data = json!({ "completedAt": created_at, "message": message, "category": category, "code": code, "retryable": retryable });
            repository
                .update_run_status(
                    &frame.run_id,
                    "failed",
                    None,
                    None,
                    Some((
                        code,
                        data["message"].as_str().unwrap_or("Agent run failed."),
                    )),
                )
                .await?;
            if let Err(error) =
                crate::routines::mark_agent_run_terminal(&repository.pool, &frame.run_id).await
            {
                tracing::warn!(agent_run_id = %frame.run_id, error_code = %error.code, "routine terminal projection failed");
            }
            Some(AgentItemPayload::Error(data.clone()))
        }
        _ => None,
    };
    if let Some(payload) = payload {
        let _ = repository
            .append_item(
                &frame.session_id,
                Some(&frame.run_id),
                frame.sequence,
                &payload,
                Some(&persistence_external_id),
            )
            .await?;
    }
    Ok(Some(PersistedRuntimeEvent {
        method: method.to_string(),
        event_id,
        data,
    }))
}

fn tool_payload(params: &Value, status: &str) -> ToolPayload {
    ToolPayload {
        tool_name: params
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string),
        tool_call_id: params
            .get("callId")
            .and_then(Value::as_str)
            .map(str::to_string),
        arguments: params.get("arguments").cloned(),
        result: params
            .get("output")
            .cloned()
            .or_else(|| params.get("error").cloned()),
        status: Some(status.into()),
    }
}
fn approval_command(tool_name: &str, arguments: Option<&Value>) -> String {
    let details = match arguments {
        Some(Value::Object(arguments)) => arguments
            .get("command")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| {
                serde_json::to_string_pretty(arguments).unwrap_or_else(|_| "{}".into())
            }),
        Some(arguments) => arguments.to_string(),
        None => "{}".into(),
    };
    sanitize_log(&format!("{tool_name} {details}"))
}

fn approval_operation_identity(params: &Value, fallback: &str) -> (String, String) {
    let provider = params.get("approvalProvider").and_then(Value::as_str);
    let remote_tool_name = params.get("approvalRemoteToolName").and_then(Value::as_str);
    match (provider, remote_tool_name) {
        (Some(provider), Some(remote_tool_name))
            if !provider.is_empty() && !remote_tool_name.is_empty() =>
        {
            (
                format!("{provider}:{remote_tool_name}"),
                format!("{provider} tool {remote_tool_name}"),
            )
        }
        _ => (fallback.to_string(), fallback.to_string()),
    }
}

fn interruption_stable_id(params: &Value, event_id: &str) -> String {
    params
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or(event_id)
        .to_string()
}

fn interruption_external_id(run_id: &str, interruption_id: &str) -> String {
    format!("interruption:{run_id}:{interruption_id}")
}

fn steering_stable_id(params: &Value, event_id: &str) -> String {
    format!(
        "steering:{}",
        params
            .get("messageId")
            .and_then(Value::as_str)
            .unwrap_or(event_id)
    )
}

pub(crate) fn sanitize_log(value: &str) -> String {
    let value = redact_bearer_tokens(value);
    let value = redact_key_tokens(&value);
    let value = redact_named_secrets(&value);
    value.chars().take(2_000).collect()
}

fn redact_named_secrets(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut output = String::with_capacity(value.len());
    let mut copy_from = 0;
    let mut cursor = 0;
    while cursor < bytes.len() {
        if matches!(bytes[cursor], b':' | b'=') {
            let mut key_end = cursor;
            while key_end > 0 && bytes[key_end - 1].is_ascii_whitespace() {
                key_end -= 1;
            }
            if key_end > 0 && matches!(bytes[key_end - 1], b'\'' | b'"') {
                key_end -= 1;
            }
            let mut key_start = key_end;
            while key_start > 0
                && matches!(bytes[key_start - 1], b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-')
            {
                key_start -= 1;
            }
            let key = value[key_start..key_end].to_ascii_lowercase();
            if matches!(
                key.as_str(),
                "authorization"
                    | "api-key"
                    | "api_key"
                    | "apikey"
                    | "cookie"
                    | "password"
                    | "secret"
            ) || key.contains("token")
            {
                let redact_through_line = matches!(key.as_str(), "authorization" | "cookie");
                let mut value_start = cursor + 1;
                while value_start < bytes.len() && bytes[value_start].is_ascii_whitespace() {
                    value_start += 1;
                }
                output.push_str(&value[copy_from..value_start]);
                if value_start < bytes.len() && matches!(bytes[value_start], b'\'' | b'"') {
                    let quote = bytes[value_start];
                    output.push(char::from(quote));
                    output.push_str("[redacted]");
                    let mut value_end = value_start + 1;
                    let mut escaped = false;
                    while value_end < bytes.len() && !matches!(bytes[value_end], b'\r' | b'\n') {
                        if bytes[value_end] == quote && !escaped {
                            break;
                        }
                        // An odd backslash run escapes the next quote; an
                        // even run leaves it as the closing delimiter.
                        escaped = bytes[value_end] == b'\\' && !escaped;
                        value_end += 1;
                    }
                    if value_end < bytes.len() && bytes[value_end] == quote {
                        output.push(char::from(quote));
                        value_end += 1;
                    }
                    copy_from = value_end;
                    cursor = value_end;
                } else {
                    output.push_str("[redacted]");
                    let mut value_end = value_start;
                    while value_end < bytes.len()
                        && !matches!(bytes[value_end], b'\r' | b'\n')
                        && (redact_through_line
                            || !matches!(bytes[value_end], b',' | b';' | b'}' | b']'))
                    {
                        value_end += 1;
                    }
                    copy_from = value_end;
                    cursor = value_end;
                }
                continue;
            }
        }
        cursor += 1;
    }
    output.push_str(&value[copy_from..]);
    output
}
fn redact_bearer_tokens(value: &str) -> String {
    const PREFIX: &str = "bearer ";
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    while cursor < value.len() {
        let tail = &value[cursor..];
        if tail
            .get(..PREFIX.len())
            .is_some_and(|candidate| candidate.eq_ignore_ascii_case(PREFIX))
        {
            output.push_str(&tail[..PREFIX.len()]);
            output.push_str("[redacted]");
            cursor += PREFIX.len();
            while cursor < value.len() {
                let character = value[cursor..]
                    .chars()
                    .next()
                    .expect("cursor remains on a character boundary");
                if !matches!(character, 'A'..='Z' | 'a'..='z' | '0'..='9' | '.' | '_' | '~' | '+' | '/' | '=' | '-')
                {
                    break;
                }
                cursor += character.len_utf8();
            }
            continue;
        }
        let character = tail
            .chars()
            .next()
            .expect("cursor remains on a character boundary");
        output.push(character);
        cursor += character.len_utf8();
    }
    output
}

fn redact_key_tokens(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut cursor = 0;
    while cursor < value.len() {
        let tail = &value[cursor..];
        let prefix_len = if tail.starts_with("osk_") {
            4
        } else if tail.starts_with("sk_") {
            3
        } else {
            0
        };
        if prefix_len > 0 {
            let mut end = cursor + prefix_len;
            while end < value.len() {
                let character = value[end..]
                    .chars()
                    .next()
                    .expect("token cursor remains on a character boundary");
                if !(character == '_' || character == '-' || character.is_ascii_alphanumeric()) {
                    break;
                }
                end += character.len_utf8();
            }
            if end - cursor >= prefix_len + 12 {
                output.push_str("[redacted]");
                cursor = end;
                continue;
            }
        }
        let character = tail
            .chars()
            .next()
            .expect("cursor remains on a character boundary");
        output.push(character);
        cursor += character.len_utf8();
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escape_regression_redacts_quotes_using_backslash_parity() {
        // All values are harmless synthetic fixtures, never real credentials.
        for quote in ['"', '\''] {
            for count in 0..=5 {
                let slashes = "\\".repeat(count);
                let (input, expected) = if count % 2 == 0 {
                    (
                        format!("password={quote}fixture{slashes}{quote}; visible=ok"),
                        format!("password={quote}[redacted]{quote}; visible=ok"),
                    )
                } else {
                    (
                        format!(
                            "password={quote}fixture{slashes}{quote}hidden-tail{quote}; visible=ok"
                        ),
                        format!("password={quote}[redacted]{quote}; visible=ok"),
                    )
                };
                assert_eq!(
                    sanitize_log(&input),
                    expected,
                    "quote={quote}, slashes={count}"
                );
            }
        }
        for value in [
            "synthetic\"suffix",
            "synthetic\\",
            "synthetic\\\"suffix",
            "é\\\\\"suffix",
        ] {
            let input = json!({ "password": value, "visible": "preserved" }).to_string();
            let sanitized: Value = serde_json::from_str(&sanitize_log(&input)).unwrap();
            assert_eq!(
                sanitized,
                json!({ "password": "[redacted]", "visible": "preserved" })
            );
        }
    }

    #[test]
    fn consumed_steering_uses_message_id_as_its_persisted_external_id() {
        assert_eq!(
            steering_stable_id(&json!({ "messageId": "message-1" }), "event-1"),
            "steering:message-1"
        );
        assert_eq!(
            steering_stable_id(&json!({}), "event-1"),
            "steering:event-1"
        );
    }
    #[test]
    fn runtime_logs_remove_credentials_and_bound_unicode_safely() {
        let sanitized = sanitize_log(&format!(
            "Authorization: Basic dXNlcjpwYXNz\nCookie: session=abc; csrf=def\nBearer live.token-123 osk_abcdefghijklmnop sk_abcdefghijklmnop\npassword=plain\napi_key='second key'\n{{\"access_token\":\"third-token\"}}\n{}",
            "é".repeat(2_100)
        ));
        assert!(!sanitized.contains("live.token-123"));
        assert!(!sanitized.contains("dXNlcjpwYXNz"));
        assert!(!sanitized.contains("csrf=def"));
        assert!(!sanitized.contains("osk_abcdefghijklmnop"));
        assert!(!sanitized.contains("sk_abcdefghijklmnop"));
        assert!(!sanitized.contains("password=plain"));
        assert!(!sanitized.contains("second key"));
        assert!(!sanitized.contains("third-token"));
        assert!(sanitized.contains("Authorization: [redacted]"));
        assert!(sanitized.contains("Cookie: [redacted]"));
        assert!(sanitized.contains("password=[redacted]"));
        assert!(sanitized.contains("api_key='[redacted]'"));
        assert!(sanitized.contains("\"access_token\":\"[redacted]\""));
        assert!(sanitized.chars().count() <= 2_000);
    }

    #[test]
    fn approval_cards_preserve_sanitized_operation_details() {
        let command = approval_command(
            "write_file",
            Some(&json!({
                "path": "/workspace/report.md",
                "content": "safe content",
                "token": "[redacted]"
            })),
        );

        assert!(command.contains("write_file"));
        assert!(command.contains("/workspace/report.md"));
        assert!(command.contains("safe content"));
        assert!(command.contains("[redacted]"));
    }

    #[test]
    fn hosted_approval_identity_preserves_provider_and_remote_tool_name() {
        let params = json!({
            "approvalProvider": "Linear",
            "approvalRemoteToolName": "save-issue.v2"
        });
        assert_eq!(
            approval_operation_identity(&params, "mcp_linear_save_issue"),
            (
                "Linear:save-issue.v2".to_string(),
                "Linear tool save-issue.v2".to_string()
            )
        );
    }

    #[test]
    fn interruption_persistence_uses_the_stable_sdk_id_across_transport_replays() {
        let params = json!({
            "id": "sdk-interruption-1",
            "callId": "sdk-tool-call-1"
        });
        let first = interruption_stable_id(&params, "transport-event-a");
        let replay = interruption_stable_id(&params, "transport-event-b");

        assert_eq!(first, replay);
        assert_eq!(
            interruption_external_id("run-1", &first),
            interruption_external_id("run-1", &replay)
        );
        assert_ne!(
            interruption_external_id("run-1", &first),
            interruption_external_id("run-2", &replay)
        );
        assert_ne!(
            interruption_stable_id(&params, "transport-event-a"),
            params["callId"]
        );
        assert_eq!(
            interruption_stable_id(&json!({}), "transport-event-c"),
            "transport-event-c"
        );
    }
}
