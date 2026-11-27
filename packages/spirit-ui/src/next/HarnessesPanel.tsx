import { useCallback, useEffect, useId, useRef, useState, type FormEvent } from "react";

import { toSpiritError, type SpiritError } from "../bridge/errors";
import type { SpiritBridge } from "../bridge/SpiritBridge";

/** One transport/protocol family, matching `spirit_core::harnesses::HarnessProtocol`. */
export type HarnessProtocol =
  | "opencode-http"
  | "acp"
  | "stream-json"
  | "codex-json"
  | "cli-pty";

/** One authentication mode, matching `spirit_core::harnesses::HarnessAuthMode`. */
export type HarnessAuthMode = "api_key" | "subscription";

/** A probe or connect destination. */
export type HarnessTarget = "local" | "home_vps";

/** One harness manifest, matching `spirit_core::harnesses::HarnessManifest`. */
export interface HarnessManifest {
  id: string;
  name: string;
  binary: string;
  install_cmd: string;
  auth_cmd: string;
  protocol: HarnessProtocol;
  supports_embedded_chat: boolean;
  auth_modes: HarnessAuthMode[];
  tos_note: string;
  docs_url: string;
  oauth_callback_port: number | null;
  builtin: boolean;
}

/** The registry listing plus safe warnings for skipped files. */
export interface HarnessListView {
  harnesses: HarnessManifest[];
  warnings: string[];
}

/** The binary-presence result. */
export type HarnessBinaryState = "found" | "missing" | "unknown";
/** The non-interactive auth result. */
export type HarnessAuthState = "configured" | "missing" | "unknown";
/** The shortest honest readiness summary. */
export type HarnessReadiness = "ready" | "install_required" | "auth_required" | "unknown";

/** A path-free probe result. */
export interface HarnessProbe {
  harness_id: string;
  target: HarnessTarget;
  protocol: HarnessProtocol;
  supports_embedded_chat: boolean;
  binary: HarnessBinaryState;
  version: string | null;
  auth: HarnessAuthState;
  auth_detail: string;
  readiness: HarnessReadiness;
  oauth_callback_port: number | null;
  requires_loopback_forward: boolean;
  warnings: string[];
}

/** A display-safe connect plan. */
export interface HarnessConnectPlanView {
  install_command: string | null;
  auth_command: string | null;
  requires_loopback_forward: boolean;
  forward_port: number | null;
}

/** A probe plus its plan. */
export interface HarnessProbeReport {
  probe: HarnessProbe;
  plan: HarnessConnectPlanView;
}

/** The result of one one-click connect. */
export interface HarnessConnectStarted {
  harness_id: string;
  target: HarnessTarget;
  auth_started: boolean;
  browser_opened: boolean;
  forward_port: number | null;
  message: string;
}

/**
 * The optional harness surface. The desktop Tauri bridge implements it; a read-only
 * bridge may omit it. Presence is the capability, exactly like the cluster and chat
 * surfaces on `SpiritBridge`.
 */
export interface HarnessesSurface {
  list(): Promise<HarnessListView>;
  saveCustom(manifest: HarnessManifest): Promise<HarnessManifest>;
  deleteCustom(id: string): Promise<boolean>;
  probe(input: { id: string; target: HarnessTarget }): Promise<HarnessProbeReport>;
  connectStart(input: { id: string; target: HarnessTarget }): Promise<HarnessConnectStarted>;
}

/** The bridge shape this panel needs; it stays a subset of `SpiritBridge`. */
export type HarnessesBridge = Pick<SpiritBridge, "kind" | "preview"> & {
  harnesses?: HarnessesSurface;
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
  protocol: HarnessProtocol;
  embeddedChat: boolean;
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
  protocol: "cli-pty",
  embeddedChat: false,
  apiKey: true,
  subscription: false,
  tosNote: "",
  docsUrl: "",
  oauthPort: "",
};

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
    protocol: form.protocol,
    supports_embedded_chat: form.embeddedChat,
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
  const [deleting, setDeleting] = useState(false);
  const alive = useRef(true);
  const ids = {
    id: useId(),
    name: useId(),
    binary: useId(),
    install: useId(),
    auth: useId(),
    protocol: useId(),
    port: useId(),
    tos: useId(),
    docs: useId(),
    embedded: useId(),
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

  function renderCard(manifest: HarnessManifest) {
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
            <span className="nx-chip nx-chip--tiny">{PROTOCOL_LABELS[manifest.protocol]}</span>
            <span
              className={`nx-chip nx-chip--tiny ${
                manifest.supports_embedded_chat ? "nx-chip--local" : "nx-chip--muted"
              }`}
            >
              {manifest.supports_embedded_chat ? "Embedded Chat" : "Worker-Only"}
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
        Choose the coding harness StackSpirit runs. Built-ins stay read-only; add your own
        with a strict local manifest. A harness is either an Embedded Chat transport or a
        Worker-Only CLI, and its note says whether it uses your API key or a subscription.
      </p>

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

      {builtins.length > 0 ? (
        <section aria-label="Built-in harnesses">
          <h3 style={{ margin: "4px 0", fontSize: 13 }}>Built-in</h3>
          <ul className="nx-block" style={{ listStyle: "none", padding: 0, margin: 0 }}>
            {builtins.map(renderCard)}
          </ul>
        </section>
      ) : null}

      {custom.length > 0 ? (
        <section aria-label="Your harnesses">
          <h3 style={{ margin: "4px 0", fontSize: 13 }}>Your harnesses</h3>
          <ul className="nx-block" style={{ listStyle: "none", padding: 0, margin: 0 }}>
            {custom.map(renderCard)}
          </ul>
        </section>
      ) : null}

      <section className="nx-card" aria-label="Add your own harness">
        <h3 style={{ margin: 0, fontSize: 14 }}>Add your own harness</h3>
        <p className="nx-form__hint">
          Saved as <code>~/.stackspirit/harnesses/&lt;id&gt;.toml</code>. The manifest has no
          key or token field; unknown fields such as <code>api_key</code> are rejected.
          StackSpirit only stores the command lines and display notes.
        </p>
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
            <label className="nx-field" htmlFor={ids.protocol}>
              Protocol
              <select
                id={ids.protocol}
                value={form.protocol}
                onChange={(event) =>
                  setForm({ ...form, protocol: event.target.value as HarnessProtocol })
                }
              >
                {(Object.keys(PROTOCOL_LABELS) as HarnessProtocol[]).map((protocol) => (
                  <option value={protocol} key={protocol}>
                    {PROTOCOL_LABELS[protocol]}
                  </option>
                ))}
              </select>
            </label>
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
          </div>

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
              id={ids.embedded}
              type="checkbox"
              checked={form.embeddedChat}
              onChange={(event) =>
                setForm({ ...form, embeddedChat: event.target.checked })
              }
            />
            <label htmlFor={ids.embedded}>Supports Embedded Chat</label>
            <span className="nx-form__hint">
              Leave unchecked for a Worker-Only harness.
            </span>
          </div>

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

          <div>
            <button
              type="submit"
              className="nx-btn"
              disabled={!desktop || !surface || saving}
              title={!desktop ? "Adding a harness is available in the desktop app only." : undefined}
            >
              {saving ? "Saving…" : "Add harness"}
            </button>
          </div>
        </form>
      </section>
    </div>
  );
}
