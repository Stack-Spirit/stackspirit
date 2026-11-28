import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type FormEvent,
} from "react";

import { toSpiritError, type SpiritError } from "../bridge/errors";
import type {
  ChatBridgeSurface,
  HarnessesSurface,
  SpiritBridge,
} from "../bridge/SpiritBridge";
import type {
  ChatHarnessGateView,
  ChatHarnessSettingsView,
  EngineStatus,
  HarnessAuthMode,
  HarnessBinaryState,
  HarnessConnectStarted,
  HarnessListView,
  HarnessManifest as WireHarnessManifest,
  HarnessProbeReport,
  HarnessProtocol,
  HarnessReadiness,
  HarnessTarget,
  KeyStatus,
  ModelChoice,
} from "../bridge/wire";
import { engineProviders, providerLabel, providerOptions } from "../domain/engine";

/**
 * The explicit capability split from `spirit_core::harnesses::HarnessSurface`.
 *
 * `bridge/wire.ts` is outside this panel's ownership, so the panel keeps a local
 * widening of the wire manifest. The extra field is optional: an older bridge that does
 * not send it falls back to `supports_embedded_chat`.
 */
export type HarnessSurface = "orchestrator_and_workers" | "workers_only_terminal";

/** A wire manifest plus the explicit harness surface when the host reports it. */
export type HarnessManifest = WireHarnessManifest & { surface?: HarnessSurface };

// The panel consumes the bridge DTOs; it does not define them. Re-exported here so the
// existing panel tests and any callers keep their imports while every type has one home
// in `bridge/wire.ts` (and `HarnessesSurface` in `bridge/SpiritBridge.ts`).
export type {
  HarnessAuthMode,
  HarnessAuthState,
  HarnessBinaryState,
  HarnessConnectPlanView,
  HarnessConnectStarted,
  HarnessListView,
  HarnessProbe,
  HarnessProbeReport,
  HarnessProtocol,
  HarnessReadiness,
  HarnessTarget,
} from "../bridge/wire";
export type { HarnessesSurface } from "../bridge/SpiritBridge";

/** The bridge shape this panel needs; it stays a subset of `SpiritBridge`. */
export type HarnessesBridge = Pick<SpiritBridge, "kind" | "preview"> & {
  harnesses?: HarnessesSurface;
  /** Provider keys are managed through the engine; absent on a read-only bridge. */
  engine?: SpiritBridge["engine"];
  /**
   * The chat default surface. Both methods must be present to show the control;
   * older and read-only bridges may omit one or both.
   */
  chat?: Partial<Pick<ChatBridgeSurface, "harnessSettings" | "setDefaultHarness">>;
};

export interface HarnessesPanelProps {
  /** Call all harness work through the bridge; the panel never invokes Tauri directly. */
  bridge: HarnessesBridge;
  /** True when a Home VPS is connected and can be probed over the existing SSH path. */
  homeConnected?: boolean;
  /** Display label for the connected Home VPS. */
  homeLabel?: string;
  /** Optional shortcut shown when no Home VPS is connected. */
  onRequestConnectServer?: () => void;
}

type ProbeEntry =
  | { state: "loading" }
  | { state: "ready"; report: HarnessProbeReport }
  | { state: "error"; error: SpiritError };

interface ConnectResult {
  name: string;
  target: HarnessTarget;
  started: HarnessConnectStarted;
}

interface FormState {
  id: string;
  name: string;
  binary: string;
  installCommand: string;
  authCommand: string;
  surface: HarnessSurface;
  protocol: HarnessProtocol;
  apiKey: boolean;
  subscription: boolean;
  tosNote: string;
  docsUrl: string;
  oauthPort: string;
}

const EMPTY_FORM: FormState = {
  id: "",
  name: "",
  binary: "",
  installCommand: "",
  authCommand: "",
  surface: "orchestrator_and_workers",
  protocol: "acp",
  apiKey: true,
  subscription: false,
  tosNote: "",
  docsUrl: "",
  oauthPort: "",
};

/** The default protocol for each surface: ACP for chat-capable, stock CLI for workers. */
function protocolForSurface(surface: HarnessSurface): HarnessProtocol {
  return surface === "workers_only_terminal" ? "cli-pty" : "acp";
}

/** The surface implied by a chosen protocol; `cli-pty` is always workers-only. */
function surfaceForProtocol(protocol: HarnessProtocol): HarnessSurface {
  return protocol === "cli-pty" ? "workers_only_terminal" : "orchestrator_and_workers";
}

function manifestSurface(manifest: HarnessManifest): HarnessSurface {
  return (
    manifest.surface ??
    (manifest.supports_embedded_chat
      ? "orchestrator_and_workers"
      : "workers_only_terminal")
  );
}

const ID_PATTERN = /^[a-z0-9](?:[a-z0-9-]{0,62}[a-z0-9])?$/;

const PROTOCOL_LABELS: Record<HarnessProtocol, string> = {
  "opencode-http": "OpenCode HTTP",
  acp: "ACP",
  "stream-json": "Stream JSON",
  "codex-json": "Codex JSON",
  "cli-pty": "CLI (pty)",
};

function probeKey(id: string, target: HarnessTarget): string {
  return `${id}:${target}`;
}

function invalidInput(message: string): SpiritError {
  return { code: "invalid_input", message, hint: null, retryable: false };
}

function targetLabel(target: HarnessTarget, homeLabel: string): string {
  return target === "local" ? "Local · this Mac" : homeLabel;
}

function readinessLabel(readiness: HarnessReadiness, binary: HarnessBinaryState): string {
  switch (readiness) {
    case "ready":
      return "Ready";
    case "install_required":
      return "Not installed";
    case "auth_required":
      return "Sign-in needed";
    default:
      return binary === "found" ? "Installed · sign-in not checked" : "Status unknown";
  }
}

function upsertKey(statuses: KeyStatus[], next: KeyStatus): KeyStatus[] {
  const rest = statuses.filter((status) => status.provider !== next.provider);
  return [...rest, next].sort((left, right) => left.provider.localeCompare(right.provider));
}

function providerKeyLabel(status: KeyStatus): string {
  if (status.state === "saved") {
    return status.last_checked_at !== null ? "Verified" : "Saved";
  }
  if (status.state === "invalid") return "Invalid key";
  if (status.state === "unverified") return "Unverified";
  return "Not connected";
}

function providerKeyTone(status: KeyStatus): string {
  if (status.state === "saved" && status.last_checked_at !== null) return "nx-chip--server";
  if (status.state === "invalid") return "nx-chip--warn";
  return "nx-chip--muted";
}

function validateForm(form: FormState): SpiritError | null {
  const id = form.id.trim();
  if (!ID_PATTERN.test(id)) {
    return invalidInput(
      "ID must be 1-64 lowercase letters, digits or hyphens, starting and ending with a letter or digit.",
    );
  }
  if (!form.name.trim()) return invalidInput("Name is required.");
  if (!form.binary.trim()) return invalidInput("Binary is required.");
  if (!form.docsUrl.trim().startsWith("https://")) {
    return invalidInput("Documentation URL must start with https://.");
  }
  if (!form.apiKey && !form.subscription) {
    return invalidInput("Choose API key, subscription, or both.");
  }
  if (form.surface === "orchestrator_and_workers" && form.protocol === "cli-pty") {
    return invalidInput(
      "Orchestrator + Workers needs ACP, OpenCode HTTP, Stream JSON or Codex JSON; CLI (pty) is for the stock terminal worker view.",
    );
  }
  if (form.surface === "workers_only_terminal" && form.protocol !== "cli-pty") {
    return invalidInput(
      "Workers Only uses CLI (pty) in the stock terminal view; choose ACP for Orchestrator + Workers.",
    );
  }
  if (!form.tosNote.trim()) {
    return invalidInput("Describe the API-key versus subscription terms note.");
  }
  const port = form.oauthPort.trim();
  if (port) {
    const parsed = Number(port);
    if (!Number.isInteger(parsed) || parsed < 1 || parsed > 65_535) {
      return invalidInput("OAuth callback port must be 1-65535.");
    }
  }
  return null;
}

function buildManifest(form: FormState): HarnessManifest {
  const authModes: HarnessAuthMode[] = [];
  if (form.apiKey) authModes.push("api_key");
  if (form.subscription) authModes.push("subscription");
  const port = form.oauthPort.trim();
  return {
    id: form.id.trim(),
    name: form.name.trim(),
    binary: form.binary.trim(),
    install_cmd: form.installCommand.trim(),
    auth_cmd: form.authCommand.trim(),
    surface: form.surface,
    protocol: form.protocol,
    supports_embedded_chat: form.surface === "orchestrator_and_workers",
    auth_modes: authModes,
    tos_note: form.tosNote.trim(),
    docs_url: form.docsUrl.trim(),
    oauth_callback_port: port ? Number(port) : null,
    builtin: false,
  };
}

function ProbeStatus({ entry }: { entry: ProbeEntry | undefined }) {
  if (!entry) {
    return <span className="nx-form__hint">Not checked.</span>;
  }
  if (entry.state === "loading") {
    return (
      <span className="nx-form__hint" role="status">
        Checking…
      </span>
    );
  }
  if (entry.state === "error") {
    return (
      <span className="nx-form__error">
        Check failed: {entry.error.message}
      </span>
    );
  }
  const { probe } = entry.report;
  return (
    <span>
      <strong>{readinessLabel(probe.readiness, probe.binary)}</strong>
      {probe.version ? <span className="nx-form__hint"> · {probe.version}</span> : null}
      <span className="nx-form__hint"> {probe.auth_detail}</span>
    </span>
  );
}

/**
 * The provider keys OpenCode uses, nested inside the default OpenCode harness card.
 *
 * OpenCode is the overarching harness; DeepSeek, OpenRouter, Anthropic, OpenAI and
 * the rest are providers inside it. A key is write-only: it goes through
 * `engine.storeKey`, the draft is cleared immediately, and no notice or error ever
 * echoes it. Without an engine surface the card says so instead of pretending.
 */
function OpenCodeProviders({ engine }: { engine?: SpiritBridge["engine"] }) {
  const [status, setStatus] = useState<EngineStatus | null>(null);
  const [keys, setKeys] = useState<KeyStatus[]>([]);
  const [models, setModels] = useState<ModelChoice[]>([]);
  const [loading, setLoading] = useState(Boolean(engine));
  const [loadError, setLoadError] = useState<SpiritError | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState<string | null>(null);
  const [removeId, setRemoveId] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<SpiritError | null>(null);
  const [reloadToken, setReloadToken] = useState(0);
  // The provider list is a bounded scroll region inside OpenCode. It stays expanded by
  // default but can be folded so every other harness remains visible in the modal.
  const [providersOpen, setProvidersOpen] = useState(true);
  const providerListId = useId();

  useEffect(() => {
    if (!engine) return;
    let cancelled = false;
    void (async () => {
      setLoading(true);
      setLoadError(null);
      try {
        const [nextStatus, nextKeys, nextModels] = await Promise.all([
          engine.status(),
          engine.keyStatus().catch(() => [] as KeyStatus[]),
          engine.models().catch(() => [] as ModelChoice[]),
        ]);
        if (cancelled) return;
        setStatus(nextStatus);
        setKeys(nextKeys);
        setModels(nextModels);
      } catch (caught) {
        if (!cancelled) setLoadError(toSpiritError(caught));
      } finally {
        if (!cancelled) setLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [engine, reloadToken]);

  const options = providerOptions(models, keys, engineProviders(status));
  const runtimeConnected = new Set(
    engineProviders(status)
      .filter((provider) => provider.connected === true)
      .map((provider) => provider.id),
  );

  function statusFor(provider: string): KeyStatus | null {
    return (
      keys.find(
        (candidate) => candidate.provider === provider && candidate.state !== "missing",
      ) ?? null
    );
  }

  async function save(provider: string): Promise<void> {
    if (!engine) return;
    const label = providerLabel(provider);
    const secret = draft.trim();
    setDraft("");
    if (!secret) {
      setError(invalidInput("Enter a provider key first."));
      return;
    }
    setBusy(provider);
    setError(null);
    setNotice(null);
    try {
      await engine.storeKey(provider, secret);
      let verified: KeyStatus;
      try {
        verified = await engine.verifyKey(provider);
      } catch {
        verified = { provider, state: "unverified", last_checked_at: null };
      }
      setKeys((current) => upsertKey(current, verified));
      setEditing(null);
      setNotice(
        verified.state === "saved"
          ? `${label} connected and verified inside OpenCode.`
          : verified.state === "invalid"
            ? `${label} rejected this key. It is saved; verify it again or replace it.`
            : `${label} key saved. The engine could not verify it yet; use Verify key to retry.`,
      );
      setReloadToken((token) => token + 1);
    } catch (caught) {
      const mapped = toSpiritError(caught);
      setError({ ...mapped, message: mapped.message.replaceAll(secret, "•••") });
    } finally {
      setBusy(null);
    }
  }

  async function verify(provider: string): Promise<void> {
    if (!engine) return;
    const label = providerLabel(provider);
    setBusy(provider);
    setError(null);
    setNotice(null);
    try {
      const checked = await engine.verifyKey(provider);
      setKeys((current) => upsertKey(current, checked));
      setNotice(
        checked.state === "saved"
          ? `${label} key verified.`
          : checked.state === "invalid"
            ? `${label} rejected this key. Replace or remove it.`
            : `${label} is saved, but the engine could not confirm it yet.`,
      );
      setReloadToken((token) => token + 1);
    } catch (caught) {
      setError(toSpiritError(caught));
    } finally {
      setBusy(null);
    }
  }

  async function remove(provider: string): Promise<void> {
    if (!engine) return;
    setBusy(provider);
    setError(null);
    setNotice(null);
    try {
      const removed = await engine.deleteKey(provider);
      setKeys((current) => upsertKey(current, removed));
      setRemoveId(null);
      setNotice(`${providerLabel(provider)} key removed from OpenCode.`);
      setReloadToken((token) => token + 1);
    } catch (caught) {
      setError(toSpiritError(caught));
    } finally {
      setBusy(null);
    }
  }

  function startEditing(provider: string): void {
    setEditing(provider);
    setDraft("");
    setError(null);
    setNotice(null);
  }

  return (
    <div className="nx-harness__providers" data-testid="harness-opencode-providers">
      <div className="nx-harness__providers-head">
        <button
          type="button"
          className="nx-harness__providers-toggle"
          aria-expanded={providersOpen}
          aria-controls={providerListId}
          onClick={() => setProvidersOpen((open) => !open)}
          data-testid="harness-opencode-providers-toggle"
        >
          <span
            className={`nx-harness__chevron${providersOpen ? " nx-harness__chevron--open" : ""}`}
            aria-hidden="true"
          >
            <svg width="12" height="12" viewBox="0 0 16 16">
              <path d="M6 4.5 10 8l-4 3.5Z" fill="currentColor" />
            </svg>
          </span>
          <span>Providers</span>
        </button>
        <span className="nx-chip nx-chip--tiny nx-chip--server">Inside OpenCode</span>
      </div>
      <p className="nx-form__hint">
        OpenCode is the default harness. Add or manage the provider keys it uses — DeepSeek,
        OpenRouter, Anthropic API, OpenAI API, Google and more. Keys stay in this Mac&apos;s
        private provider-key file and are never sent to your server.
      </p>

      {providersOpen ? (
        <div
          id={providerListId}
          className="nx-harness__provider-scroll"
          data-testid="harness-opencode-provider-scroll"
          role="region"
          aria-label="OpenCode provider list"
          tabIndex={0}
        >
          {!engine ? (
        <div data-testid="harness-opencode-providers-unavailable">
          <p className="nx-form__hint">
            Provider keys are managed by the running engine in the desktop app. This bridge has
            no engine surface, so StackSpirit does not pretend to add or verify a key here.
          </p>
          <div className="nx-harness__provider-chips">
            {providerOptions([], [], []).map((provider) => (
              <span className="nx-chip nx-chip--tiny nx-chip--muted" key={provider.id}>
                {provider.label}
              </span>
            ))}
          </div>
        </div>
      ) : loading ? (
        <p className="nx-form__hint" role="status">
          Reading OpenCode providers…
        </p>
      ) : loadError ? (
        <p className="nx-form__error" role="alert">
          {loadError.message}
          {loadError.hint ? <span className="nx-form__hint"> {loadError.hint}</span> : null}
        </p>
      ) : (
        <>
          {status ? (
            <p className="nx-form__hint" data-testid="harness-opencode-engine-state">
              Engine {status.state.state}
              {status.selected_model ? ` · current model ${status.selected_model.label}` : ""}
            </p>
          ) : null}
          <ul className="nx-provider__list" aria-label="OpenCode model providers">
            {options.map((provider) => {
              const saved = statusFor(provider.id);
              const connected = saved !== null || runtimeConnected.has(provider.id);
              const isBusy = busy === provider.id;
              const removing = removeId === provider.id;
              return (
                <li
                  className="nx-provider-row"
                  key={provider.id}
                  data-testid={`opencode-provider-${provider.id}`}
                >
                  <div className="nx-provider-row__head">
                    <div className="nx-provider-row__name">
                      <strong>{provider.label}</strong>
                      <span
                        className={
                          connected
                            ? "nx-chip nx-chip--tiny nx-chip--server"
                            : "nx-chip nx-chip--tiny nx-chip--muted"
                        }
                      >
                        {connected ? "Connected" : "Not connected"}
                      </span>
                      {saved ? (
                        <span className={`nx-chip nx-chip--tiny ${providerKeyTone(saved)}`}>
                          {providerKeyLabel(saved)}
                        </span>
                      ) : null}
                    </div>
                    <div className="nx-provider-row__actions">
                      {saved ? (
                        <>
                          <button
                            type="button"
                            className="nx-btn nx-btn--quiet nx-btn--sm"
                            onClick={() => void verify(provider.id)}
                            disabled={isBusy}
                            aria-label={`Verify ${provider.label} key`}
                          >
                            {isBusy ? "Verifying…" : "Verify key"}
                          </button>
                          <button
                            type="button"
                            className="nx-btn nx-btn--quiet nx-btn--sm"
                            onClick={() =>
                              editing === provider.id
                                ? setEditing(null)
                                : startEditing(provider.id)
                            }
                            disabled={isBusy}
                            aria-expanded={editing === provider.id}
                            aria-label={`Replace ${provider.label} key`}
                          >
                            Replace key
                          </button>
                          {removing ? (
                            <>
                              <button
                                type="button"
                                className="nx-btn nx-btn--quiet nx-btn--sm"
                                onClick={() => void remove(provider.id)}
                                disabled={isBusy}
                              >
                                {isBusy ? "Removing…" : "Confirm remove"}
                              </button>
                              <button
                                type="button"
                                className="nx-btn nx-btn--quiet nx-btn--sm"
                                onClick={() => setRemoveId(null)}
                                disabled={isBusy}
                              >
                                Cancel
                              </button>
                            </>
                          ) : (
                            <button
                              type="button"
                              className="nx-btn nx-btn--quiet nx-btn--sm"
                              onClick={() => setRemoveId(provider.id)}
                              disabled={isBusy}
                              aria-label={`Remove ${provider.label} key`}
                            >
                              Remove key
                            </button>
                          )}
                        </>
                      ) : (
                        <button
                          type="button"
                          className="nx-btn nx-btn--sm"
                          onClick={() =>
                            editing === provider.id
                              ? setEditing(null)
                              : startEditing(provider.id)
                          }
                          disabled={isBusy}
                          aria-expanded={editing === provider.id}
                          aria-label={`Connect ${provider.label}`}
                        >
                          {editing === provider.id ? "Cancel" : "Connect"}
                        </button>
                      )}
                    </div>
                  </div>

                  {editing === provider.id ? (
                    <form
                      className="nx-provider-form"
                      onSubmit={(event) => {
                        event.preventDefault();
                        void save(provider.id);
                      }}
                    >
                      <label className="nx-field">
                        <span>API key for {provider.label}</span>
                        <input
                          type="password"
                          value={draft}
                          onChange={(event) => setDraft(event.target.value)}
                          aria-label={`API key for ${provider.label}`}
                          autoComplete="new-password"
                          spellCheck={false}
                          placeholder="Paste the provider key"
                        />
                      </label>
                      <div className="nx-provider-form__actions">
                        <button type="submit" className="nx-btn nx-btn--sm" disabled={isBusy}>
                          {isBusy ? "Saving…" : "Save key"}
                        </button>
                        <button
                          type="button"
                          className="nx-btn nx-btn--quiet nx-btn--sm"
                          onClick={() => setEditing(null)}
                          disabled={isBusy}
                        >
                          Cancel
                        </button>
                        <span className="nx-form__hint">
                          Stored locally; OpenCode uses it for {provider.label}.
                        </span>
                      </div>
                    </form>
                  ) : null}
                </li>
              );
            })}
          </ul>
        </>
      )}
        </div>
      ) : null}

      {notice ? (
        <p className="nx-form__hint" role="status" data-testid="harness-opencode-provider-notice">
          {notice}
        </p>
      ) : null}
      {error ? (
        <p className="nx-form__error" role="alert">
          {error.message}
        </p>
      ) : null}
    </div>
  );
}

interface ChatTransportOption {
  id: string;
  name: string;
}

interface ChatTransportGateCopy {
  token: string;
  tone: "ready" | "warn";
  title: string;
  detail: string;
  canUseOpenCode: boolean;
}

interface ChatTransportControlProps {
  manifests: HarnessManifest[];
  loadSettings: () => Promise<ChatHarnessSettingsView>;
  saveDefault: (harnessId: string) => Promise<ChatHarnessSettingsView>;
}

/**
 * The chat-selectable registry entries: OpenCode plus every orchestrator manifest that
 * is not a stock `cli-pty` terminal. Workers-only entries never appear here, even when a
 * malformed manifest also claims embedded chat.
 */
function chatTransportOptions(manifests: HarnessManifest[]): ChatTransportOption[] {
  const options: ChatTransportOption[] = [
    { id: "opencode", name: "OpenCode" },
  ];
  const seen = new Set(["opencode"]);
  for (const manifest of manifests) {
    if (seen.has(manifest.id)) continue;
    if (manifestSurface(manifest) !== "orchestrator_and_workers") continue;
    if (manifest.supports_embedded_chat === false) continue;
    if (manifest.protocol === "cli-pty") continue;
    seen.add(manifest.id);
    options.push({ id: manifest.id, name: manifest.name });
  }
  return options;
}

function chatTransportOptionLabel(option: ChatTransportOption): string {
  return option.id === "opencode"
    ? "OpenCode · ready"
    : `${option.name} · harness_not_wired`;
}

/**
 * Fixed, safe copy for one gate. It repeats the stable token and never includes a path,
 * endpoint, port, token value or native id. Only OpenCode can recover a non-ready gate;
 * an unreadable settings file gets no write action at all.
 */
function chatTransportGateCopy(
  gate: ChatHarnessGateView,
  nameFor: (harnessId: string) => string,
): ChatTransportGateCopy {
  switch (gate.state) {
    case "ready":
      return {
        token: gate.state,
        tone: "ready",
        title: `${nameFor(gate.harness_id)} is ready for new chats.`,
        detail:
          "New app-owned local conversations will use it. This control does not start a chat.",
        canUseOpenCode: false,
      };
    case "harness_not_wired":
      return {
        token: gate.state,
        tone: "warn",
        title: `${nameFor(gate.harness_id)} is not wired for chat yet.`,
        detail:
          "Its chat adapter is not wired yet, so a new setup chat will return " +
          "harness_not_wired and start no process. The selection is saved for planning " +
          "only; no harness chat is live.",
        canUseOpenCode: true,
      };
    case "harness_missing":
      return {
        token: gate.state,
        tone: "warn",
        title: "The saved chat harness is missing.",
        detail: `${nameFor(gate.harness_id)} is not in the harness registry. StackSpirit did not change the saved default; choose another default explicitly.`,
        canUseOpenCode: true,
      };
    case "harness_worker_only":
      return {
        token: gate.state,
        tone: "warn",
        title: "That harness is workers-only.",
        detail: `${nameFor(gate.harness_id)} cannot back the embedded chat. It is never chat-selectable, and the saved default was not changed. Choose another default explicitly.`,
        canUseOpenCode: true,
      };
    case "harness_protocol_unsupported":
      return {
        token: gate.state,
        tone: "warn",
        title: "That harness protocol cannot back the embedded chat.",
        detail: `${nameFor(gate.harness_id)} uses ${PROTOCOL_LABELS[gate.protocol]}. The saved default was not changed; choose another default explicitly.`,
        canUseOpenCode: true,
      };
    case "chat_settings_unreadable": {
      const { problem } = gate;
      return {
        token: gate.state,
        tone: "warn",
        title:
          problem.reason === "unsupported_version"
            ? "The chat settings file uses an unsupported version."
            : "The chat settings file cannot be read.",
        detail:
          problem.reason === "unsupported_version"
            ? `The file uses settings version ${problem.found}; this app supports version ${problem.supported}. StackSpirit will not overwrite it. Fix or remove the file explicitly, then reopen this panel.`
            : "StackSpirit could not read it and will not overwrite it. Fix or remove the file explicitly, then reopen this panel.",
        canUseOpenCode: false,
      };
    }
  }
}

/**
 * Settings → Harnesses → Chat transport.
 *
 * It reads the persisted default once, offers only chat-eligible orchestrator manifests,
 * and saves one explicit selection through the bridge. Every save renders the returned
 * gate; nothing here starts a chat, opens a transport or silently falls back to OpenCode.
 */
function ChatTransportControl({
  manifests,
  loadSettings,
  saveDefault,
}: ChatTransportControlProps) {
  const [settings, setSettings] = useState<ChatHarnessSettingsView | null>(null);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<SpiritError | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<SpiritError | null>(null);
  const savingRef = useRef(false);
  const alive = useRef(true);
  const selectId = useId();
  const options = useMemo(() => chatTransportOptions(manifests), [manifests]);

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    loadSettings()
      .then((next) => {
        if (cancelled) return;
        setSettings(next);
        setLoading(false);
      })
      .catch((caught: unknown) => {
        if (cancelled) return;
        setLoadError(toSpiritError(caught));
        setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [loadSettings]);

  async function choose(harnessId: string) {
    if (savingRef.current) return;
    savingRef.current = true;
    setSaving(true);
    setSaveError(null);
    try {
      const next = await saveDefault(harnessId);
      if (!alive.current) return;
      setSettings(next);
    } catch (caught) {
      if (!alive.current) return;
      setSaveError(toSpiritError(caught));
    } finally {
      savingRef.current = false;
      if (alive.current) setSaving(false);
    }
  }

  const nameFor = useCallback(
    (harnessId: string) =>
      options.find((option) => option.id === harnessId)?.name ?? harnessId,
    [options],
  );
  const gate = settings?.gate ?? null;
  const gateCopy = gate ? chatTransportGateCopy(gate, nameFor) : null;
  const gateSelection =
    gate?.state === "ready" || gate?.state === "harness_not_wired"
      ? gate.harness_id
      : "";
  const selectedValue = options.some((option) => option.id === gateSelection)
    ? gateSelection
    : "";
  const selectedName = gateSelection ? nameFor(gateSelection) : "";
  const selectedManifest =
    manifests.find((manifest) => manifest.id === gateSelection) ?? null;
  const canChoose = gate !== null && gate.state !== "chat_settings_unreadable";
  const chipTone =
    gateCopy?.tone === "warn"
      ? "nx-chip nx-chip--tiny nx-chip--warn"
      : "nx-chip nx-chip--tiny nx-chip--server";

  return (
    <section
      className="nx-card"
      aria-label="Chat transport"
      data-testid="harness-chat-transport"
    >
      <div className="nx-block__head">
        <div>
          <h3 style={{ margin: 0, fontSize: 14 }}>Chat transport</h3>
          <p className="nx-form__hint" data-testid="harness-chat-transport-scope">
            Applies to new app-owned local conversations only. VOWP{" "}
            <code>global</code>/<code>project</code> chats stay OpenCode, and existing
            sessions do not move.
          </p>
        </div>
        {gateCopy ? (
          <span className={chipTone} data-testid="harness-chat-transport-state">
            {gateCopy.token}
          </span>
        ) : null}
      </div>

      {loading ? (
        <p className="nx-form__hint" role="status">
          Reading the chat transport setting…
        </p>
      ) : loadError ? (
        <p className="nx-form__error nx-form__error--block" role="alert">
          Could not read the chat transport setting. {loadError.message}
          {loadError.hint ? (
            <span className="nx-form__hint"> {loadError.hint}</span>
          ) : null}
        </p>
      ) : settings && gateCopy ? (
        <>
          {canChoose ? (
            <>
              {selectedName ? (
                <p
                  className="nx-form__hint"
                  data-testid="harness-chat-transport-selected"
                >
                  Selected for new conversations: <strong>{selectedName}</strong>
                </p>
              ) : null}
              <label className="nx-field" htmlFor={selectId}>
                Default chat harness
                <select
                  id={selectId}
                  data-testid="harness-chat-transport-select"
                  value={selectedValue}
                  disabled={saving}
                  onChange={(event) => {
                    const harnessId = event.target.value;
                    if (harnessId) void choose(harnessId);
                  }}
                >
                  {selectedValue ? null : (
                    <option value="" disabled>
                      Choose a harness…
                    </option>
                  )}
                  {options.map((option) => (
                    <option value={option.id} key={option.id}>
                      {chatTransportOptionLabel(option)}
                    </option>
                  ))}
                </select>
              </label>
              {selectedManifest?.tos_note ? (
                <p className="nx-form__hint">{selectedManifest.tos_note}</p>
              ) : null}
            </>
          ) : null}

          <div
            className={gateCopy.tone === "warn" ? "nx-banner nx-banner--warn" : "nx-banner"}
            role="status"
            data-testid="harness-chat-transport-gate"
          >
            <strong>{gateCopy.title}</strong>
            <p className="nx-form__hint" style={{ margin: "4px 0 0" }}>
              {gateCopy.detail}
            </p>
          </div>

          {gateCopy.canUseOpenCode || saving ? (
            <div className="nx-settings__actions">
              {gateCopy.canUseOpenCode ? (
                <button
                  type="button"
                  className="nx-btn nx-btn--sm"
                  disabled={saving}
                  onClick={() => void choose("opencode")}
                  data-testid="harness-chat-transport-use-opencode"
                >
                  Use OpenCode
                </button>
              ) : null}
              {saving ? (
                <span
                  className="nx-form__hint"
                  role="status"
                  data-testid="harness-chat-transport-busy"
                >
                  Saving the chat default…
                </span>
              ) : null}
            </div>
          ) : null}

          {saveError ? (
            <p className="nx-form__error nx-form__error--block" role="alert">
              {saveError.message}
              {saveError.hint ? (
                <span className="nx-form__hint"> {saveError.hint}</span>
              ) : null}
            </p>
          ) : null}
        </>
      ) : null}
    </section>
  );
}

/**
 * The Universal Harness Registry panel. It lists the built-in presets plus the owner's
 * custom manifests, shows Embedded Chat versus Worker-Only and the API-key versus
 * subscription terms note, probes Local and the Home VPS, and offers the desktop
 * one-click Connect. The actual command, SSH and loopback-forward work lives in
 * `spirit-core` behind the host bridge; this file never opens a terminal or a tunnel.
 *
 * In a non-desktop bridge (`http-sse`, or a preview) the list is still readable when a
 * surface is present, but Connect is disabled and labelled desktop-only.
 */
export function HarnessesPanel({
  bridge,
  homeConnected = false,
  homeLabel = "Home VPS",
  onRequestConnectServer,
}: HarnessesPanelProps) {
  const surface = bridge.harnesses;
  const desktop = bridge.kind === "tauri" && !bridge.preview;
  // A stable pair of functions keeps the settings effect to one read per mount even
  // when the registry list updates. The chat object is never mutated here.
  const chatTransport = useMemo(() => {
    if (!desktop) return null;
    const chat = bridge.chat;
    const loadSettings = chat?.harnessSettings;
    const saveDefault = chat?.setDefaultHarness;
    if (!chat || !loadSettings || !saveDefault) return null;
    return {
      loadSettings: () => loadSettings.call(chat),
      saveDefault: (harnessId: string) => saveDefault.call(chat, harnessId),
    };
  }, [bridge.chat, desktop]);
  const [list, setList] = useState<HarnessListView | null>(null);
  const [loading, setLoading] = useState(Boolean(surface));
  const [listError, setListError] = useState<SpiritError | null>(null);
  const [reloadToken, setReloadToken] = useState(0);
  const [probes, setProbes] = useState<Record<string, ProbeEntry>>({});
  const [connectBusy, setConnectBusy] = useState<string | null>(null);
  const [connectResult, setConnectResult] = useState<ConnectResult | null>(null);
  const [connectError, setConnectError] = useState<SpiritError | null>(null);
  const [form, setForm] = useState<FormState>(EMPTY_FORM);
  const [formError, setFormError] = useState<SpiritError | null>(null);
  const [formNotice, setFormNotice] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [deleteId, setDeleteId] = useState<string | null>(null);
  const [addOpen, setAddOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const alive = useRef(true);
  const ids = {
    id: useId(),
    name: useId(),
    binary: useId(),
    install: useId(),
    auth: useId(),
    surface: useId(),
    protocol: useId(),
    port: useId(),
    tos: useId(),
    docs: useId(),
    apiKey: useId(),
    subscription: useId(),
  };

  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false;
    };
  }, []);

  useEffect(() => {
    if (!surface) {
      setLoading(false);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setListError(null);
    surface
      .list()
      .then((next) => {
        if (cancelled) return;
        setProbes({});
        setList(next);
        setLoading(false);
      })
      .catch((caught: unknown) => {
        if (cancelled) return;
        setListError(toSpiritError(caught));
        setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [surface, reloadToken]);

  const probeOne = useCallback(
    async (id: string, target: HarnessTarget) => {
      if (!surface) return;
      const key = probeKey(id, target);
      setProbes((current) => ({ ...current, [key]: { state: "loading" } }));
      try {
        const report = await surface.probe({ id, target });
        if (!alive.current) return;
        setProbes((current) => ({ ...current, [key]: { state: "ready", report } }));
      } catch (caught) {
        if (!alive.current) return;
        setProbes((current) => ({
          ...current,
          [key]: { state: "error", error: toSpiritError(caught) },
        }));
      }
    },
    [surface],
  );

  useEffect(() => {
    if (!surface || !list) return;
    const harnesses = list.harnesses;
    let cancelled = false;
    async function probeAll() {
      for (const manifest of harnesses) {
        if (cancelled) return;
        await probeOne(manifest.id, "local");
        if (homeConnected) {
          if (cancelled) return;
          await probeOne(manifest.id, "home_vps");
        }
      }
    }
    void probeAll();
    return () => {
      cancelled = true;
    };
  }, [surface, list, homeConnected, probeOne]);

  async function connect(manifest: HarnessManifest, target: HarnessTarget) {
    if (!surface) return;
    const key = probeKey(manifest.id, target);
    setConnectBusy(key);
    setConnectError(null);
    setConnectResult(null);
    try {
      const started = await surface.connectStart({ id: manifest.id, target });
      if (!alive.current) return;
      setConnectResult({ name: manifest.name, target, started });
    } catch (caught) {
      if (!alive.current) return;
      setConnectError(toSpiritError(caught));
    } finally {
      if (alive.current) setConnectBusy(null);
    }
  }

  async function submit(event: FormEvent) {
    event.preventDefault();
    if (!surface || saving) return;
    const validation = validateForm(form);
    if (validation) {
      setFormError(validation);
      setFormNotice(null);
      return;
    }
    const manifest = buildManifest(form);
    setSaving(true);
    setFormError(null);
    setFormNotice(null);
    try {
      const saved = await surface.saveCustom(manifest);
      if (!alive.current) return;
      setForm(EMPTY_FORM);
      setFormNotice(`${saved.name} saved. Its next probe uses the new manifest.`);
      setReloadToken((token) => token + 1);
    } catch (caught) {
      if (!alive.current) return;
      setFormError(toSpiritError(caught));
    } finally {
      if (alive.current) setSaving(false);
    }
  }

  async function removeCustom(id: string) {
    if (!surface || deleting) return;
    setDeleting(true);
    setFormError(null);
    try {
      await surface.deleteCustom(id);
      if (!alive.current) return;
      setDeleteId(null);
      setFormNotice("Harness removed.");
      setReloadToken((token) => token + 1);
    } catch (caught) {
      if (!alive.current) return;
      setFormError(toSpiritError(caught));
    } finally {
      if (alive.current) setDeleting(false);
    }
  }

  const builtins = list?.harnesses.filter((manifest) => manifest.builtin) ?? [];
  const custom = list?.harnesses.filter((manifest) => !manifest.builtin) ?? [];
  const defaultHarness = builtins.find((manifest) => manifest.id === "opencode") ?? null;
  const remainingBuiltins = defaultHarness
    ? builtins.filter((manifest) => manifest.id !== "opencode")
    : builtins;
  const grouped = [...remainingBuiltins, ...custom];
  const orchestratorHarnesses = grouped.filter(
    (manifest) => manifestSurface(manifest) === "orchestrator_and_workers",
  );
  const workersOnlyHarnesses = grouped.filter(
    (manifest) => manifestSurface(manifest) === "workers_only_terminal",
  );

  function renderCard(manifest: HarnessManifest) {
    const surface = manifestSurface(manifest);
    const local = probes[probeKey(manifest.id, "local")];
    const home = probes[probeKey(manifest.id, "home_vps")];
    const localMissing =
      local?.state === "ready" && local.report.probe.binary === "missing";
    const homeMissing = home?.state === "ready" && home.report.probe.binary === "missing";
    const localBusy = connectBusy === probeKey(manifest.id, "local");
    const homeBusy = connectBusy === probeKey(manifest.id, "home_vps");
    const forwardPort =
      (home?.state === "ready"
        ? home.report.plan.forward_port ?? home.report.probe.oauth_callback_port
        : null) ??
      (local?.state === "ready" ? local.report.plan.forward_port : null);
    const localInstall =
      local?.state === "ready" && local.report.probe.binary === "missing"
        ? local.report.plan.install_command ?? manifest.install_cmd
        : null;
    const homeInstall =
      home?.state === "ready" && home.report.probe.binary === "missing"
        ? home.report.plan.install_command ?? manifest.install_cmd
        : null;
    const install = localInstall ?? homeInstall;
    const removing = deleteId === manifest.id;
    return (
      <li className="nx-card" key={manifest.id} data-testid={`harness-${manifest.id}`}>
        <div className="nx-block__head">
          <div>
            <h3 style={{ margin: 0, fontSize: 14 }}>{manifest.name}</h3>
            <div className="nx-card__label">
              {manifest.id} · <code>{manifest.binary}</code>
            </div>
          </div>
          <span style={{ display: "inline-flex", flexWrap: "wrap", gap: 6 }}>
            {manifest.id === "opencode" ? (
              <span
                className="nx-chip nx-chip--tiny nx-chip--server"
                data-testid="harness-opencode-default"
              >
                Default harness
              </span>
            ) : null}
            <span className="nx-chip nx-chip--tiny">{PROTOCOL_LABELS[manifest.protocol]}</span>
            <span
              className={`nx-chip nx-chip--tiny ${
                surface === "orchestrator_and_workers" ? "nx-chip--local" : "nx-chip--muted"
              }`}
              data-testid={`harness-${manifest.id}-surface`}
            >
              {surface === "orchestrator_and_workers"
                ? "Orchestrator + Workers"
                : "Workers only"}
            </span>
            <span
              className={`nx-chip nx-chip--tiny ${
                surface === "orchestrator_and_workers" ? "nx-chip--local" : "nx-chip--muted"
              }`}
            >
              {surface === "orchestrator_and_workers" ? "Embedded Chat" : "Worker-Only"}
            </span>
            <span className="nx-chip nx-chip--tiny nx-chip--muted">
              {manifest.builtin ? "Built-in" : "Custom"}
            </span>
          </span>
        </div>

        <div style={{ display: "flex", flexWrap: "wrap", gap: 6 }}>
          {manifest.auth_modes.map((mode) => (
            <span className="nx-chip nx-chip--tiny" key={mode}>
              {mode === "api_key" ? "API key · BYOK" : "Subscription sign-in"}
            </span>
          ))}
          <a
            className="nx-form__hint"
            href={manifest.docs_url}
            target="_blank"
            rel="noreferrer"
          >
            Documentation
          </a>
        </div>

        <p className="nx-form__hint" data-testid={`harness-${manifest.id}-tos`}>
          {manifest.tos_note}
        </p>

        {manifest.id === "opencode" ? <OpenCodeProviders engine={bridge.engine} /> : null}

        <dl className="nx-form" style={{ gap: 6, margin: 0 }}>
          <div>
            <dt className="nx-form__label">Local · this Mac</dt>
            <dd style={{ margin: 0 }} data-testid={`harness-${manifest.id}-local`}>
              <ProbeStatus entry={local} />
            </dd>
          </div>
          <div>
            <dt className="nx-form__label">{homeLabel}</dt>
            <dd style={{ margin: 0 }} data-testid={`harness-${manifest.id}-home`}>
              {homeConnected ? (
                <ProbeStatus entry={home} />
              ) : (
                <span className="nx-form__hint">Not connected.</span>
              )}
            </dd>
          </div>
        </dl>

        {forwardPort ? (
          <p className="nx-form__hint" data-testid={`harness-${manifest.id}-forward`}>
            Home VPS sign-in uses an automatic temporary loopback forward on
            127.0.0.1:{forwardPort}. No terminal or manual tunnel is opened.
          </p>
        ) : null}
        {install ? (
          <p className="nx-form__hint" data-testid={`harness-${manifest.id}-install`}>
            Install {manifest.binary} on the target first: <code>{install}</code>
          </p>
        ) : null}

        <div style={{ display: "flex", flexWrap: "wrap", gap: 8, alignItems: "center" }}>
          <button
            type="button"
            className="nx-btn nx-btn--sm"
            disabled={!desktop || !surface || localBusy || localMissing}
            title={
              !desktop
                ? "Connect is available in the desktop app only."
                : localMissing
                  ? `Install ${manifest.binary} first.`
                  : undefined
            }
            onClick={() => void connect(manifest, "local")}
          >
            {localBusy ? "Starting…" : "Connect on this Mac"}
          </button>
          <button
            type="button"
            className="nx-btn nx-btn--sm nx-btn--quiet"
            disabled={
              !desktop || !surface || !homeConnected || homeBusy || homeMissing
            }
            title={
              !desktop
                ? "Connect is available in the desktop app only."
                : !homeConnected
                  ? "Connect a saved Home VPS first."
                  : homeMissing
                    ? `Install ${manifest.binary} on the Home VPS first.`
                    : undefined
            }
            onClick={() => void connect(manifest, "home_vps")}
          >
            {homeBusy ? "Starting…" : `Connect on ${homeLabel}`}
          </button>
          {!desktop ? (
            <span className="nx-chip nx-chip--tiny nx-chip--muted">Desktop only</span>
          ) : null}
          {!manifest.builtin ? (
            removing ? (
              <>
                <button
                  type="button"
                  className="nx-btn nx-btn--sm nx-btn--quiet"
                  disabled={deleting}
                  onClick={() => void removeCustom(manifest.id)}
                >
                  {deleting ? "Removing…" : "Confirm remove"}
                </button>
                <button
                  type="button"
                  className="nx-btn nx-btn--sm nx-btn--quiet"
                  disabled={deleting}
                  onClick={() => setDeleteId(null)}
                >
                  Cancel
                </button>
              </>
            ) : (
              <button
                type="button"
                className="nx-btn nx-btn--sm nx-btn--quiet"
                onClick={() => setDeleteId(manifest.id)}
              >
                Remove
              </button>
            )
          ) : null}
        </div>
      </li>
    );
  }

  return (
    <div
      className="nx-block"
      role="region"
      aria-label="Harness registry"
      data-testid="harnesses-panel"
    >
      <div className="nx-block__head">
        <h2>Harnesses</h2>
        <span style={{ display: "inline-flex", gap: 8, alignItems: "center" }}>
          {!desktop ? (
            <span className="nx-chip nx-chip--tiny nx-chip--muted">Desktop only</span>
          ) : null}
          <button
            type="button"
            className="nx-btn nx-btn--quiet nx-btn--sm"
            disabled={!surface || loading}
            onClick={() => setReloadToken((token) => token + 1)}
          >
            Refresh
          </button>
        </span>
      </div>

      <p className="nx-form__lede">
        OpenCode is the default overarching harness, and its provider keys (DeepSeek,
        OpenRouter, Anthropic, OpenAI and more) live inside its card. The catalog is split
        by surface: official ACP / Codex app-server harnesses can back the embedded chat,
        while consumer-CLI harnesses stay in the stock terminal worker view. Add your own
        with a strict local manifest.
      </p>

      <p className="nx-form__hint" data-testid="harness-chat-transport-note">
        Connect installs and signs in a harness. The reusable ACP client core exists, but
        the embedded chat still runs on OpenCode until harness-transport wiring lands;
        choosing another chat transport is not live yet.
      </p>

      {chatTransport ? (
        <ChatTransportControl
          manifests={list?.harnesses ?? []}
          loadSettings={chatTransport.loadSettings}
          saveDefault={chatTransport.saveDefault}
        />
      ) : null}

      {!surface ? (
        <p className="nx-banner nx-banner--warn" role="status">
          Harness connect is available in the desktop app. This bridge has no harness
          surface, so StackSpirit does not pretend to probe or connect a harness.
        </p>
      ) : null}

      {listError ? (
        <p className="nx-form__error nx-form__error--block" role="alert">
          {listError.message}
          {listError.hint ? <span className="nx-form__hint"> {listError.hint}</span> : null}
        </p>
      ) : null}

      {list && list.warnings.length > 0 ? (
        <ul className="nx-banner nx-banner--warn" aria-label="Harness registry warnings">
          {list.warnings.map((warning) => (
            <li key={warning}>{warning}</li>
          ))}
        </ul>
      ) : null}

      {connectError ? (
        <p className="nx-form__error nx-form__error--block" role="alert">
          {connectError.message}
          {connectError.hint ? (
            <span className="nx-form__hint"> {connectError.hint}</span>
          ) : null}
        </p>
      ) : null}
      {connectResult ? (
        <div className="nx-banner" role="status" data-testid="harness-connect-result">
          <strong>{connectResult.name}</strong> ·{" "}
          {targetLabel(connectResult.target, homeLabel)}. {connectResult.started.message}
          {connectResult.started.forward_port
            ? ` Temporary loopback forward on 127.0.0.1:${connectResult.started.forward_port}.`
            : ""}
        </div>
      ) : null}

      {loading ? (
        <p className="nx-form__hint" role="status">
          Reading the harness registry…
        </p>
      ) : null}

      {!homeConnected && surface ? (
        <p className="nx-form__hint">
          Local status is probed on this Mac. Connect a saved Home VPS to see its status and
          use the automatic loopback forward for its OAuth callback.{" "}
          {onRequestConnectServer ? (
            <button
              type="button"
              className="nx-btn nx-btn--quiet nx-btn--sm"
              onClick={onRequestConnectServer}
            >
              Connect a Home VPS
            </button>
          ) : null}
        </p>
      ) : null}

      {defaultHarness ? (
        <section aria-label="Default harness" data-testid="harnesses-default-section">
          <h3 style={{ margin: "4px 0", fontSize: 13 }}>Default harness · OpenCode</h3>
          <ul className="nx-block" style={{ listStyle: "none", padding: 0, margin: 0 }}>
            {renderCard(defaultHarness)}
          </ul>
        </section>
      ) : null}

      {orchestratorHarnesses.length > 0 ? (
        <section
          aria-label="Orchestrator and workers harnesses"
          data-testid="harnesses-orchestrator-section"
        >
          <h3 style={{ margin: "4px 0", fontSize: 13 }}>
            Orchestrator + Workers (ACP / Embedded Chat)
          </h3>
          <p className="nx-form__hint">
            OpenCode HTTP, official ACP and Codex app-server harnesses. Connect installs
            and signs in; OpenCode remains the live embedded-chat transport for now.
          </p>
          <ul className="nx-block" style={{ listStyle: "none", padding: 0, margin: 0 }}>
            {orchestratorHarnesses.map(renderCard)}
          </ul>
        </section>
      ) : null}

      {workersOnlyHarnesses.length > 0 ? (
        <section
          aria-label="Workers only harnesses"
          data-testid="harnesses-workers-section"
        >
          <h3 style={{ margin: "4px 0", fontSize: 13 }}>
            Workers Only (Stock CLI Terminal View)
          </h3>
          <p className="nx-form__hint">
            These keep their own consumer sign-in in a stock CLI. StackSpirit never wraps
            that OAuth session in its embedded chat.
          </p>
          <ul className="nx-block" style={{ listStyle: "none", padding: 0, margin: 0 }}>
            {workersOnlyHarnesses.map(renderCard)}
          </ul>
        </section>
      ) : null}

      <section className="nx-card" aria-label="Add your own harness" data-testid="harness-add-custom-card">
        <div className="nx-block__head">
          <div>
            <h3 style={{ margin: 0, fontSize: 14 }}>Add your own harness</h3>
            <p className="nx-form__hint">
              Saved as <code>~/.stackspirit/harnesses/&lt;id&gt;.toml</code>. The manifest has no
              key or token field; unknown fields such as <code>api_key</code> are rejected.
              StackSpirit only stores the command lines and display notes.
            </p>
          </div>
          <button
            type="button"
            className="nx-btn nx-btn--quiet nx-btn--sm nx-harness__add"
            aria-expanded={addOpen}
            onClick={() => setAddOpen((open) => !open)}
            data-testid="harness-add-custom"
          >
            <span aria-hidden="true">+</span>{" "}
            {addOpen ? "Close" : "Add custom harness"}
          </button>
        </div>
        {addOpen ? (
          <form className="nx-form" onSubmit={(event) => void submit(event)}>
          <div className="nx-form__grid">
            <label className="nx-field" htmlFor={ids.id}>
              ID
              <input
                id={ids.id}
                value={form.id}
                onChange={(event) => setForm({ ...form, id: event.target.value })}
                placeholder="my-harness"
                spellCheck={false}
                autoComplete="off"
              />
            </label>
            <label className="nx-field" htmlFor={ids.binary}>
              Binary
              <input
                id={ids.binary}
                value={form.binary}
                onChange={(event) => setForm({ ...form, binary: event.target.value })}
                placeholder="my-harness"
                spellCheck={false}
                autoComplete="off"
              />
            </label>
          </div>

          <label className="nx-field" htmlFor={ids.name}>
            Name
            <input
              id={ids.name}
              value={form.name}
              onChange={(event) => setForm({ ...form, name: event.target.value })}
              placeholder="My Harness"
              autoComplete="off"
            />
          </label>

          <div className="nx-form__grid">
            <label className="nx-field" htmlFor={ids.surface}>
              Surface
              <select
                id={ids.surface}
                value={form.surface}
                onChange={(event) => {
                  const nextSurface = event.target.value as HarnessSurface;
                  setForm({
                    ...form,
                    surface: nextSurface,
                    protocol: protocolForSurface(nextSurface),
                  });
                }}
              >
                <option value="orchestrator_and_workers">
                  Orchestrator + Workers (ACP / Embedded Chat)
                </option>
                <option value="workers_only_terminal">
                  Workers Only (Stock CLI Terminal View)
                </option>
              </select>
            </label>
            <label className="nx-field" htmlFor={ids.protocol}>
              Protocol
              <select
                id={ids.protocol}
                value={form.protocol}
                onChange={(event) => {
                  const nextProtocol = event.target.value as HarnessProtocol;
                  setForm({
                    ...form,
                    protocol: nextProtocol,
                    surface: surfaceForProtocol(nextProtocol),
                  });
                }}
              >
                {(Object.keys(PROTOCOL_LABELS) as HarnessProtocol[]).map((protocol) => (
                  <option value={protocol} key={protocol}>
                    {PROTOCOL_LABELS[protocol]}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <label className="nx-field" htmlFor={ids.port}>
            OAuth callback port
            <input
              id={ids.port}
              type="number"
              min={1}
              max={65_535}
              value={form.oauthPort}
              onChange={(event) => setForm({ ...form, oauthPort: event.target.value })}
              placeholder="optional"
              autoComplete="off"
            />
          </label>

          <label className="nx-field" htmlFor={ids.install}>
            Install command (shown, never run automatically)
            <input
              id={ids.install}
              value={form.installCommand}
              onChange={(event) =>
                setForm({ ...form, installCommand: event.target.value })
              }
              placeholder="npm install -g my-harness"
              spellCheck={false}
              autoComplete="off"
            />
          </label>

          <label className="nx-field" htmlFor={ids.auth}>
            Sign-in command (shell-free)
            <input
              id={ids.auth}
              value={form.authCommand}
              onChange={(event) => setForm({ ...form, authCommand: event.target.value })}
              placeholder="my-harness login"
              spellCheck={false}
              autoComplete="off"
            />
          </label>

          <div className="nx-form__check">
            <input
              id={ids.apiKey}
              type="checkbox"
              checked={form.apiKey}
              onChange={(event) => setForm({ ...form, apiKey: event.target.checked })}
            />
            <label htmlFor={ids.apiKey}>API key (BYOK)</label>
          </div>
          <div className="nx-form__check">
            <input
              id={ids.subscription}
              type="checkbox"
              checked={form.subscription}
              onChange={(event) =>
                setForm({ ...form, subscription: event.target.checked })
              }
            />
            <label htmlFor={ids.subscription}>Subscription sign-in</label>
          </div>

          <label className="nx-field" htmlFor={ids.tos}>
            API-key versus subscription terms note
            <textarea
              id={ids.tos}
              value={form.tosNote}
              onChange={(event) => setForm({ ...form, tosNote: event.target.value })}
              rows={3}
              placeholder="API-key use follows the provider's API terms. Subscription sign-in follows its consumer terms and may be restricted."
            />
          </label>

          <label className="nx-field" htmlFor={ids.docs}>
            Documentation URL
            <input
              id={ids.docs}
              value={form.docsUrl}
              onChange={(event) => setForm({ ...form, docsUrl: event.target.value })}
              placeholder="https://example.com/docs"
              spellCheck={false}
              autoComplete="off"
            />
          </label>

          {formError ? (
            <p className="nx-form__error nx-form__error--block" role="alert">
              {formError.message}
              {formError.hint ? (
                <span className="nx-form__hint"> {formError.hint}</span>
              ) : null}
            </p>
          ) : null}
          {formNotice ? (
            <p className="nx-form__hint" role="status">
              {formNotice}
            </p>
          ) : null}

          <div className="nx-settings__actions">
            <button
              type="submit"
              className="nx-btn"
              disabled={!desktop || !surface || saving}
              title={!desktop ? "Adding a harness is available in the desktop app only." : undefined}
            >
              {saving ? "Saving…" : "Add harness"}
            </button>
            <button
              type="button"
              className="nx-btn nx-btn--quiet"
              onClick={() => setAddOpen(false)}
              disabled={saving}
            >
              Cancel
            </button>
          </div>
        </form>
        ) : null}
      </section>
    </div>
  );
}
