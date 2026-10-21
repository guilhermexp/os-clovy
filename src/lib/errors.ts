import { type MessageKey, t, type TranslateParams } from "../i18n/translate";

/** One wording for a code. `match` picks it by the native message (exact
 * text, or a pattern whose groups fill `names` as params); an entry without
 * `match` is the code's default. */
type ErrorWording = { key: MessageKey; match?: string | RegExp; names?: string[] };

/** Stable native error codes (Rust `AppError`, dictation helper events) whose
 * English message is replaced by interface-language copy. Rust and Swift keep
 * writing English; the code is the contract and the message is only the
 * fallback for codes without an entry. Each key's English text is the native
 * message, so English reads exactly as before. */
const ERROR_CODE_MESSAGES: Record<string, MessageKey | ErrorWording[]> = {
  // Dictation helper (mac-dictation-helper/main.swift)
  recording_start_busy: "hud.error.recordingStartBusy",
  already_listening: "hud.error.alreadyListening",
  not_listening: "hud.error.notListening",
  microphone_permission_missing: "hud.error.microphonePermissionMissing",
  audio_start_failed: "hud.error.audioStartFailed",
  missing_recording: "hud.error.missingRecording",
  empty_transcript: "hud.error.emptyTranscript",
  paste_target_unavailable: "hud.error.pasteManually",
  accessibility_permission_missing: "hud.error.pasteManually",
  pasteboard_write_failed: "hud.error.clipboardWriteFailed",
  dictation_recovery_copy_failed: "hud.error.recoveryCopyFailed",
  dictation_recovery_clipboard: "hud.error.recoveryClipboard",
  invalid_input: "hud.error.helperCommandInvalid",
  unknown_command: "hud.error.helperCommandInvalid",
  invalid_shortcut: "hud.error.shortcutInvalid",
  shortcut_monitor_unavailable: "hud.error.shortcutMonitorUnavailable",
  shortcut_fn_chord_unsupported: "hud.error.shortcutFnChordUnsupported",
  shortcut_register_failed: "hud.error.shortcutRegisterFailed",
  escape_cancel_unavailable: "hud.error.escapeCancelUnavailable",
  // Desktop dictation pipeline (src-tauri/src/dictation.rs)
  no_speech: "hud.error.noSpeech",
  dictation_helper_unavailable: [
    {
      key: "hud.error.recordingUnsupported",
      match: "Dictation recording is not supported on this platform.",
    },
    { key: "hud.error.helperUnavailable" },
  ],
  dictation_helper_missing: "hud.error.helperUnavailable",
  dictation_helper_start_failed: "hud.error.helperUnavailable",
  dictation_helper_write_failed: "hud.error.helperUnavailable",
  dictation_helper_command_invalid: "hud.error.helperCommandInvalid",
  dictation_helper_owner_unrecordable: "hud.error.helperRestarting",
  dictation_helper_reap_failed: "hud.error.helperRestarting",
  dictation_helper_orphan_stuck: "hud.error.helperRestarting",
  dictation_recovery_unavailable: "hud.error.recoveryUnavailable",
  dictation_response_invalid: "hud.error.responseInvalid",
  dictation_audio_without_text: "hud.error.audioWithoutText",
  dictation_provider_not_configured: "hud.error.providerNotConfigured",
  dictation_cleanup_timeout: "hud.error.cleanupTimeout",
  dictation_cleanup_invalid: "hud.error.cleanupInstruction",
  provider_response_invalid: "hud.error.cleanupNoText",
  dictation_history_save_failed: "hud.error.historySaveFailed",
  dictation_recording_path_missing: "hud.error.recordingPathMissing",
  hotkey_registration_failed: "hud.error.hotkeyRegistrationFailed",
  dictation_settings_unavailable: "hud.error.settingsUnavailable",
  dictation_settings_save_failed: "hud.error.settingsSaveFailed",
  dictation_shortcut_incomplete: "hud.error.shortcutIncomplete",
  dictation_shortcut_duplicate: "hud.error.shortcutDuplicate",
  dictation_shortcut_modifier_required: [
    {
      key: "hud.error.shortcutModifierRequiredWindows",
      match: "Shortcut must include at least one modifier key (Ctrl, Alt, Shift, Win).",
    },
    { key: "hud.error.shortcutModifierRequired" },
  ],
  dictation_shortcut_unsupported: [
    {
      key: "hud.error.shortcutDoublePressWindows",
      match: "Double-press shortcuts are not supported on Windows.",
    },
    {
      key: "hud.error.shortcutFnWindows",
      match: "The Fn key is not supported for Windows dictation shortcuts.",
    },
    {
      key: "hud.error.shortcutModifierOnlyWindows",
      match: "Modifier-only shortcuts are not supported on Windows.",
    },
    {
      key: "hud.error.shortcutKeyRequired",
      match: "Shortcut must include a supported non-modifier key.",
    },
    { key: "hud.error.shortcutPlatformUnsupported" },
  ],
  dictation_composer_registration_forbidden: "hud.error.composerUnavailable",
  dictation_composer_registration_unavailable: "hud.error.composerUnavailable",
  dictation_composer_request_invalid: "hud.error.composerUnavailable",
  dictation_composer_window_unavailable: "hud.error.composerUnavailable",
  dictation_take_invalid: "hud.error.composerUnavailable",
  // Clovy API client (src-tauri/src/clovy_api.rs)
  insufficient_credits: "lib.errors.insufficientCredits",
  empty_response: "lib.errors.emptyResponse",
  clovy_api_response_invalid: "lib.errors.streamInvalid",
  audio_artifact_missing: "lib.errors.audioArtifactMissing",
  transcription_empty: "lib.errors.transcriptionEmpty",
  local_model_not_configured: [
    {
      key: "lib.errors.localModelIdNotConfigured",
      match: "Configure a local model endpoint and model ID first.",
    },
    { key: "lib.errors.localModelNotConfigured" },
  ],
  local_model_invalid: [
    {
      key: "lib.errors.localModelInvalid",
      match: "The local model selected for this session is invalid. Choose the model again.",
    },
  ],
  local_model_unavailable: "lib.errors.localModelUnavailable",
  local_model_empty: [
    { key: "lib.errors.cliModelEmpty", match: "The CLI did not return generated note text." },
    { key: "lib.errors.localModelEmpty" },
  ],
  local_model_failed: [
    {
      key: "lib.errors.localModelStatus",
      match: /^Local model returned status (\d+)\.$/,
      names: ["status"],
    },
  ],
  remote_model_invalid: "lib.errors.remoteModelInvalid",
  auto_resolved_model_invalid: "lib.errors.autoModelInvalid",
  // Note pipeline (src-tauri/src/domain/processing.rs)
  transcription_already_running: [
    {
      key: "lib.errors.transcriptionSpanAlreadyRunning",
      match: "This saved-audio span is already being transcribed.",
    },
    { key: "lib.errors.transcriptionAlreadyRunning" },
  ],
  note_transcript_cleanup_timeout: "lib.errors.noteCleanupTimeout",
  // Own engines: agent CLIs and endpoints (src-tauri/src/llm/)
  llm_provider_not_local: "lib.errors.llm.notLocal",
  llm_cli_not_installed: [
    {
      key: "lib.errors.llm.cliNotInstalled",
      match: /^(.+) is not installed on this Mac\.$/,
      names: ["cli"],
    },
  ],
  llm_cli_tools_not_disabled: [
    {
      key: "lib.errors.llm.cliToolsNotDisabled",
      match: /^(.+) cannot run with its tools turned off,/,
      names: ["cli"],
    },
  ],
  llm_cli_failed: [
    {
      key: "lib.errors.llm.cliFailed",
      match: /^(.+?) failed: ([\s\S]*)$/,
      names: ["cli", "detail"],
    },
  ],
  llm_endpoint_not_found: "lib.errors.llm.endpointNotFound",
  llm_endpoint_failed: [
    {
      key: "lib.errors.llm.endpointUnreachable",
      match: "Could not reach the endpoint. Check the URL and that the server is running.",
    },
    { key: "lib.errors.llm.endpointUnreadable", match: "Could not read the endpoint response." },
    { key: "lib.errors.llm.endpointKeyRejected", match: "The endpoint rejected the API key." },
    {
      key: "lib.errors.llm.endpointStatusDetail",
      match: /^The endpoint returned status (\d+): ([\s\S]*)$/,
      names: ["status", "detail"],
    },
    {
      key: "lib.errors.llm.endpointStatus",
      match: /^The endpoint returned status (\d+)$/,
      names: ["status"],
    },
  ],
  llm_invalid_output: [
    { key: "lib.errors.llm.endpointNoText", match: "The endpoint returned no text." },
    {
      key: "lib.errors.llm.endpointSchema",
      match: "The endpoint did not return JSON matching the schema.",
    },
    {
      key: "lib.errors.llm.providerSchema",
      match: "The provider did not return JSON matching the schema.",
    },
    { key: "lib.errors.llm.cliEmpty", match: /^(.+) returned an empty answer\.$/, names: ["cli"] },
    {
      key: "lib.errors.llm.cliInvalidJson",
      match: /^(.+) did not return valid JSON\.$/,
      names: ["cli"],
    },
  ],
  llm_timeout: "lib.errors.llm.timeout",
  llm_structured_output_insufficient: "lib.errors.llm.structuredOutput",
  llm_activity_provider_missing: "lib.errors.llm.activityProviderMissing",
  llm_provider_not_allowed: [
    {
      key: "lib.errors.llm.activityNeverClovyApi",
      match: "Activity features never use Clovy API. Choose one of your own providers or none.",
    },
    { key: "lib.errors.llm.chooseProvider" },
  ],
  llm_keychain_unavailable: "lib.errors.llm.keychainUnavailable",
  llm_endpoint_key_invalid: "lib.errors.llm.endpointKeyInvalid",
  llm_endpoint_model_required: "lib.errors.llm.endpointModelRequired",
  llm_endpoint_name_invalid: "lib.errors.llm.endpointNameInvalid",
};

/** `clovy_request_failed` carries the Clovy API's reason as its message: a
 * snake_case machine code from the envelope, or a network error's text. */
const REQUEST_FAILED_CODES = new Set(["clovy_request_failed", "june_request_failed"]);

const REQUEST_FAILED_REASONS: Record<string, MessageKey> = {
  upstream_provider_failed: "lib.errors.reason.upstreamProviderFailed",
  metering_provider_failed: "notes.failure.billingTemporarilyUnavailable",
  authorization_denied: "notes.failure.serviceBusy",
  concurrency_cap_exceeded: "notes.failure.serviceBusy",
  server_busy: "notes.failure.serviceBusy",
  insufficient_credits: "lib.errors.insufficientCredits",
  no_speech: "hud.error.noSpeech",
  transcript_empty: "lib.errors.reason.transcriptEmpty",
  dictation_text_empty: "lib.errors.reason.transcriptEmpty",
  audio_too_large: "lib.errors.reason.audioTooLarge",
  file_too_large: "lib.errors.reason.audioTooLarge",
  payload_too_large: "lib.errors.reason.payloadTooLarge",
  unsupported_audio_format: "lib.errors.reason.audioFormatUnsupported",
  audio_required: "lib.errors.reason.audioRequired",
  duration_required: "lib.errors.reason.requestUnreadable",
  model_capability_unavailable: "lib.errors.reason.modelCapabilityUnavailable",
  model_required: "lib.errors.reason.modelRequired",
  string_too_long: "lib.errors.reason.textTooLong",
  prompt_required: "lib.errors.reason.promptRequired",
  invalid_chat_completion_body: "lib.errors.reason.requestUnreadable",
  json_too_deep: "lib.errors.reason.requestUnreadable",
  multipart_invalid: "lib.errors.reason.requestUnreadable",
  generation_failed: "lib.errors.reason.generationFailed",
  sharing_unavailable: "lib.errors.sharingUnavailable",
  share_not_found: "lib.errors.shareNotFound",
};

/** reqwest's transport errors ("error sending request for url (...)"). */
const NETWORK_ERROR =
  /^(error sending request|error decoding response body|error following redirect|request or response body error|builder error|operation timed out|Couldn't reach Clovy\.$)/i;

/** Clovy API provider reasons (`groq_api_key_rejected`), see
 * `provider_error` in clovy_api.rs. */
const PROVIDER_NAMES: Record<string, string> = {
  groq: "Groq",
  openrouter: "OpenRouter",
  fal: "fal",
  tavily: "Tavily",
};
const PROVIDER_REASONS: Record<string, MessageKey> = {
  api_key_invalid: "lib.errors.provider.keyInvalid",
  api_key_too_long: "lib.errors.provider.keyInvalid",
  api_key_rejected: "lib.errors.provider.keyRejected",
  api_key_required: "lib.errors.provider.keyRequired",
  api_key_not_accepted: "lib.errors.provider.keyNotAccepted",
  api_key_insufficient_credits: "lib.errors.provider.keyNoCredits",
  api_key_model_unavailable: "lib.errors.provider.keyModelUnavailable",
  not_configured: "lib.errors.provider.notConfigured",
  model_unavailable: "lib.errors.provider.modelUnavailable",
  usage_limit_exceeded: "lib.errors.provider.usageLimit",
  request_inaccessible: "lib.errors.provider.requestInaccessible",
};

function providerWording(code: string): string | undefined {
  const match = code.match(/^(groq|openrouter|fal|tavily)_(.+)$/);
  const key = match ? PROVIDER_REASONS[match[2]] : undefined;
  return match && key ? t(key, { name: PROVIDER_NAMES[match[1]] }) : undefined;
}

function requestFailedWording(message: string): string | undefined {
  const reason = message.trim();
  const reasonKey = REQUEST_FAILED_REASONS[reason];
  if (reasonKey) return t(reasonKey);
  const provider = providerWording(reason);
  if (provider) return provider;
  if (NETWORK_ERROR.test(reason)) return t("lib.errors.unreachable");
  // Any other machine reason is never shown raw.
  if (/^[a-z0-9_]+$/.test(reason)) return t("lib.errors.requestFailed");
  return undefined;
}

function codeWording(
  code: string,
  message: string,
  params: TranslateParams | undefined,
): string | undefined {
  const entry = ERROR_CODE_MESSAGES[code];
  if (typeof entry === "string") return t(entry, params);
  if (entry) {
    for (const wording of entry) {
      if (wording.match === undefined) return t(wording.key, params);
      if (typeof wording.match === "string") {
        if (wording.match === message.trim()) return t(wording.key, params);
        continue;
      }
      const groups = message.trim().match(wording.match);
      if (!groups) continue;
      const extracted: TranslateParams = {};
      (wording.names ?? []).forEach((name, index) => {
        extracted[name] = groups[index + 1] ?? "";
      });
      return t(wording.key, { ...params, ...extracted });
    }
    return undefined;
  }
  if (REQUEST_FAILED_CODES.has(code)) return requestFailedWording(message);
  return providerWording(code);
}

/** Interface-language copy for a native error code, or the native message
 * when Clovy has no wording for it. `params` fills placeholders such as
 * `{shortcut}`. */
export function messageForErrorCode(
  code: string | undefined,
  fallback: string,
  params?: TranslateParams,
): string {
  if (!code) return fallback;
  return codeWording(code, fallback, params) ?? fallback;
}

/** The native message of a thrown value, untranslated: for logic that
 * matches the wire text (a share's `share_not_found`, the Max plan gate). */
export function rawMessageFromError(err: unknown) {
  if (err && typeof err === "object" && "message" in err) {
    return String((err as { message: unknown }).message);
  }
  return String(err);
}

/** Human-readable message from a thrown value — Tauri command errors arrive
 * as objects with a `message` field (translated by `code` when Clovy knows
 * it), everything else falls back to String. */
export function messageFromError(err: unknown) {
  if (err && typeof err === "object" && "message" in err) {
    return messageForErrorCode(errorCode(err), rawMessageFromError(err));
  }
  return String(err);
}

/** Stable error code from a thrown Tauri `AppError` (`{ code, message }`),
 * or undefined for anything without one. Lets callers branch on a specific
 * failure (e.g. "provider_api_key_rejected") instead of matching message text. */
export function errorCode(err: unknown): string | undefined {
  if (err && typeof err === "object" && "code" in err) {
    const code = (err as { code: unknown }).code;
    return typeof code === "string" ? code : undefined;
  }
  return undefined;
}

/** Whether a share request failed because the share is unknown, revoked, or
 * not owned by the caller. The Clovy API collapses all of these to a 404 whose
 * message is `share_not_found` (non-enumeration); the structured code is the
 * generic `clovy_request_failed`, so we match the message. Lets the owner
 * dialog treat a definitively-gone share differently from a transient error. */
export function isShareNotFoundError(err: unknown): boolean {
  return rawMessageFromError(err) === "share_not_found";
}

/** Human-readable share command error. The Clovy API's share endpoints answer
 * with bare machine codes as messages (`sharing_unavailable` when the server
 * has no share database configured, `share_not_found` for unknown or revoked
 * shares); user-facing surfaces must not leak those raw codes. */
export function describeShareError(err: unknown): string {
  const message = rawMessageFromError(err);
  if (message === "sharing_unavailable") {
    return t("lib.errors.sharingUnavailable");
  }
  if (message === "share_not_found") {
    return t("lib.errors.shareNotFound");
  }
  return messageFromError(err);
}
