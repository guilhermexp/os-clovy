import { type MessageKey, t, type TranslateParams } from "../i18n/translate";

/** Stable native error codes (Rust `AppError`, dictation helper events) whose
 * English message is replaced by interface-language copy. Rust and Swift keep
 * writing English; the code is the contract and the message is only the
 * fallback for codes without an entry. */
const ERROR_CODE_MESSAGES: Record<string, MessageKey> = {
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
  dictation_helper_unavailable: "hud.error.helperUnavailable",
  not_signed_in: "hud.error.notSignedIn",
  dictation_recovery_unavailable: "hud.error.recoveryUnavailable",
  dictation_response_invalid: "hud.error.responseInvalid",
  dictation_audio_without_text: "hud.error.audioWithoutText",
  dictation_provider_not_configured: "hud.error.providerNotConfigured",
  dictation_cleanup_timeout: "hud.error.cleanupTimeout",
  dictation_history_save_failed: "hud.error.historySaveFailed",
  dictation_recording_path_missing: "hud.error.recordingPathMissing",
  hotkey_registration_failed: "hud.error.hotkeyRegistrationFailed",
  // Home conversation (src-tauri/src/clovy_api.rs)
  home_chat_failed: "lib.errors.homeChatFailed",
  home_chat_current_research_required: "lib.errors.homeChatResearchRequired",
  home_chat_empty: "lib.errors.homeChatEmpty",
  home_chat_invalid: "lib.errors.homeChatInvalid",
};

/** Interface-language copy for a native error code, or the native message
 * when the code has no entry. `params` fills placeholders such as
 * `{shortcut}`. */
export function messageForErrorCode(
  code: string | undefined,
  fallback: string,
  params?: TranslateParams,
): string {
  const key = code ? ERROR_CODE_MESSAGES[code] : undefined;
  return key ? t(key, params) : fallback;
}

/** Human-readable message from a thrown value — Tauri command errors arrive
 * as objects with a `message` field (translated by `code` when Clovy knows
 * it), everything else falls back to String. */
export function messageFromError(err: unknown) {
  if (err && typeof err === "object" && "message" in err) {
    return messageForErrorCode(errorCode(err), String((err as { message: unknown }).message));
  }
  return String(err);
}

/** Stable error code from a thrown Tauri `AppError` (`{ code, message }`),
 * or undefined for anything without one. Lets callers branch on a specific
 * failure (e.g. "referrals_unavailable") instead of matching message text. */
export function errorCode(err: unknown): string | undefined {
  if (err && typeof err === "object" && "code" in err) {
    const code = (err as { code: unknown }).code;
    return typeof code === "string" ? code : undefined;
  }
  return undefined;
}

/** Whether an error means buying credits is gated behind the Max plan. Robust
 * to every shape the gate can reach us in: the structured Rust AppError code
 * (`top_up_requires_max`, mapped in os_accounts.rs from the accounts
 * envelope), the raw numeric envelope code (3002) should a payload ever pass
 * through unmapped, and the canonical message ("Buying credits requires the
 * Max plan.") as a last resort. */
export function isTopUpRequiresMaxError(err: unknown): boolean {
  if (errorCode(err) === "top_up_requires_max") return true;
  if (numericErrorCode(err) === 3002) return true;
  return /requires the max plan/i.test(messageFromError(err));
}

/** Numeric error code from a raw accounts envelope (`error_code`) or any
 * error object carrying a numeric code field, or undefined when absent. */
function numericErrorCode(err: unknown): number | undefined {
  if (!err || typeof err !== "object") return undefined;
  const shape = err as { code?: unknown; error_code?: unknown; errorCode?: unknown };
  for (const value of [shape.error_code, shape.errorCode, shape.code]) {
    if (typeof value === "number") return value;
  }
  return undefined;
}

/** Whether a share request failed because the share is unknown, revoked, or
 * not owned by the caller. The Clovy API collapses all of these to a 404 whose
 * message is `share_not_found` (non-enumeration); the structured code is the
 * generic `clovy_request_failed`, so we match the message. Lets the owner
 * dialog treat a definitively-gone share differently from a transient error. */
export function isShareNotFoundError(err: unknown): boolean {
  return messageFromError(err) === "share_not_found";
}

/** Human-readable share command error. The Clovy API's share endpoints answer
 * with bare machine codes as messages (`sharing_unavailable` when the server
 * has no share database configured, `share_not_found` for unknown or revoked
 * shares); user-facing surfaces must not leak those raw codes. */
export function describeShareError(err: unknown): string {
  const message = messageFromError(err);
  if (message === "sharing_unavailable") {
    return t("lib.errors.sharingUnavailable");
  }
  if (message === "share_not_found") {
    return t("lib.errors.shareNotFound");
  }
  return message;
}

/** Whether an error message means the user's balance ran out. String match is
 * intentional and a known weakness — billing failures reach us as plain text
 * from several layers (Tauri commands and upstream provider errors),
 * none of which carry a structured code today. The patterns cover the Clovy
 * API's friendly message and the raw provider error
 * (`... 'error_code': 4301, 'message': 'insufficient_credits'`). */
export function isInsufficientCreditsMessage(message?: string) {
  if (!message) return false;
  return /out of credits|insufficient credits|insufficient_credits|balance is too low/i.test(
    message,
  );
}
