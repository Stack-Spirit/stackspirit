import type { PasswordAuthOutcome, PasswordAuthUnsupported } from "../bridge/wire";

/** Plain-language copy for why `PasswordAuthentication no` was not applied. */
export const PASSWORD_AUTH_REASON: Record<PasswordAuthUnsupported, string> = {
  sudo_unavailable: "this server has no usable sudo, so sshd was not changed",
  no_passwordless_sudo: "this account has no passwordless sudo, so sshd was not changed",
  sshd_unavailable: "sshd was not available for StackSpirit to check",
  drop_in_unsupported: "this server has no sshd_config.d drop-in directory",
  drop_in_ignored: "this server's sshd ignores drop-in configuration",
  reload_unavailable: "StackSpirit could not reload sshd safely",
};

/** Honest copy for the optional password-login step after an onboarding install. */
export function passwordAuthSummary(outcome: PasswordAuthOutcome): string {
  switch (outcome.state) {
    case "disabled":
      return "Password login is now turned off; the StackSpirit key has a verified key-only login.";
    case "not_requested":
      return "Password login is still on because you asked to keep it. The new key works.";
    case "not_supported":
      return `Password login is still on: ${PASSWORD_AUTH_REASON[outcome.reason]} (${outcome.reason}). The new key works.`;
    case "failed":
      return `Password login is still on: the change failed (${outcome.detail}). The new key works; you can retry from Settings.`;
  }
}

/** Honest copy for the Settings retry, which has no new install to describe. */
export function passwordAuthSettingsSummary(outcome: PasswordAuthOutcome): string {
  switch (outcome.state) {
    case "disabled":
      return "Password login is now off. StackSpirit verified a fresh key-only login before reporting this.";
    case "not_requested":
      return "Password login is still on; this retry did not ask to change it.";
    case "not_supported":
      return `Password login is still on: ${PASSWORD_AUTH_REASON[outcome.reason]} (${outcome.reason}).`;
    case "failed":
      return `Password login is still on: the change failed (${outcome.detail}).`;
  }
}
