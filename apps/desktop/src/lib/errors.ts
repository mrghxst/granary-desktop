import type { TFunction } from "i18next";
import { isCommandError } from "./silo";

/** Every `code` the host can return (crates/silo-api/src/error.rs `ApiError::code`, the player's
 * `ControllerError::code` and `Notice::Error` codes, plus `no_host`). */
export const ERROR_CODES = [
  "invalid_url",
  "tls_expired",
  "tls_not_yet_valid",
  "tls_untrusted",
  "tls_name_mismatch",
  "tls_revoked",
  "tls_error",
  "unreachable",
  "timeout",
  "redirected",
  "not_silo",
  "unsupported_api",
  "account_required",
  "account_disabled",
  "password_change_required",
  "profile_verification_required",
  "rate_limited",
  "client_upgrade_required",
  "unauthorized",
  "forbidden",
  "not_found",
  "unavailable",
  "server_error",
  "http_error",
  "invalid_credentials",
  "signed_out",
  "not_signed_in",
  "unknown_server",
  "wrong_pin",
  "decode",
  "credentials",
  "internal",
  "storage",
  "profile_required",
  "bad_artwork_url",
  "invalid_request",
  "no_host",
  // Player (apps/desktop/src-tauri/src/playback): command errors …
  "not_playable",
  "playback_unsupported",
  "player_unavailable",
  "player_error",
  // … and playback notices.
  "player_shutdown",
  "playback_session_ended",
  "installation_changed",
  "stream_failed",
  "reconnecting",
] as const;

/** A translated, user-facing message for a command error; unknown codes fall back to a generic text. The host's
 * (English, technical) message is only ever shown as the `{{reason}}` detail of the player codes. */
export function errorMessage(t: TFunction, e: unknown): string {
  if (!isCommandError(e)) return t("errors.unknown");
  if ((ERROR_CODES as readonly string[]).includes(e.code)) {
    return t(`errors.${e.code}`, {
      retryAfter: e.retryAfter ?? "",
      status: e.status ?? "",
      reason: e.message.replace(/^(not playable|player): /, ""),
    });
  }
  return t("errors.unknown");
}
