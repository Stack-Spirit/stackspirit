import { useMemo, useState } from "react";

export type ToolCategoryId =
  | "deploy_cloud"
  | "code_issues"
  | "communication_calendar"
  | "databases"
  | "browser_docs"
  | "custom";

export type ToolTargetChoice = "both" | "local" | "home";
export type ToolSideState =
  "not_connected" | "connecting" | "connected" | "needs_attention";
export type McpClientId =
  "claude-desktop" | "claude-code" | "antigravity" | "cursor" | "opencode";
export type McpMode = "local" | "remote";

export interface ToolCatalogEntry {
  id: string;
  name: string;
  category: ToolCategoryId;
  description: string;
  cli?: string;
  mcp?: "stdio" | "http" | "sse";
  custom?: boolean;
}

export interface ToolSideStatus {
  mac: ToolSideState;
  home: ToolSideState;
}

export interface CustomToolDraft {
  kind: "cli" | "mcp";
  name: string;
  /** CLI binary name, or MCP stdio command. */
  command: string;
  /** MCP transport. */
  transport: "stdio" | "http" | "sse";
  /** MCP HTTP/SSE URL. Never holds a secret. */
  url: string;
}

export const CATEGORIES: ReadonlyArray<{ id: ToolCategoryId; label: string }> =
  [
    { id: "deploy_cloud", label: "Deploy & Cloud" },
    { id: "code_issues", label: "Code & Issues" },
    { id: "communication_calendar", label: "Communication & Calendar" },
    { id: "databases", label: "Databases" },
    { id: "browser_docs", label: "Browser & Docs" },
    { id: "custom", label: "Custom" },
  ];

/** Mirrors the spirit-core built-in presets; no secret or account value. */
export const BUILTIN_TOOLS: ReadonlyArray<ToolCatalogEntry> = [
  {
    id: "github",
    name: "GitHub",
    category: "code_issues",
    description: "Repositories, pull requests and issues.",
    cli: "gh",
    mcp: "http",
  },
  {
    id: "vercel",
    name: "Vercel",
    category: "deploy_cloud",
    description: "Deploy and inspect Vercel projects.",
    cli: "vercel",
    mcp: "http",
  },
  {
    id: "cloudflare",
    name: "Cloudflare",
    category: "deploy_cloud",
    description: "Workers, Pages, DNS and R2 through wrangler.",
    cli: "wrangler",
    mcp: "http",
  },
  {
    id: "stripe",
    name: "Stripe",
    category: "deploy_cloud",
    description: "Payments, customers and webhooks.",
    cli: "stripe",
    mcp: "http",
  },
  {
    id: "supabase",
    name: "Supabase",
    category: "databases",
    description: "Postgres, auth and storage projects.",
    cli: "supabase",
    mcp: "http",
  },
  {
    id: "linear",
    name: "Linear",
    category: "code_issues",
    description: "Issues, projects and cycles.",
    mcp: "http",
  },
  {
    id: "slack",
    name: "Slack",
    category: "communication_calendar",
    description: "Read and post in Slack channels.",
    mcp: "http",
  },
  {
    id: "gmail",
    name: "Gmail",
    category: "communication_calendar",
    description: "Search and draft email.",
    mcp: "http",
  },
  {
    id: "google-calendar",
    name: "Google Calendar",
    category: "communication_calendar",
    description: "Read and create calendar events.",
    mcp: "http",
  },
  {
    id: "notion",
    name: "Notion",
    category: "browser_docs",
    description: "Search and edit Notion pages.",
    mcp: "http",
  },
  {
    id: "sentry",
    name: "Sentry",
    category: "code_issues",
    description: "Errors, releases and performance traces.",
    mcp: "http",
  },
  {
    id: "postgres",
    name: "Postgres",
    category: "databases",
    description: "Query a Postgres database.",
    mcp: "stdio",
  },
  {
    id: "playwright",
    name: "Playwright",
    category: "browser_docs",
    description: "Drive a real browser for testing and scraping.",
    mcp: "stdio",
  },
  {
    id: "context7",
    name: "Context7",
    category: "browser_docs",
    description: "Up-to-date library documentation.",
    mcp: "stdio",
  },
  {
    id: "docker",
    name: "Docker",
    category: "deploy_cloud",
    description: "Build and run containers.",
    cli: "docker",
  },
  {
    id: "aws",
    name: "AWS",
    category: "deploy_cloud",
    description: "Amazon Web Services through the aws CLI.",
    cli: "aws",
  },
  {
    id: "gcloud",
    name: "Google Cloud",
    category: "deploy_cloud",
    description: "Google Cloud through the gcloud CLI.",
    cli: "gcloud",
  },
  {
    id: "flyio",
    name: "Fly.io",
    category: "deploy_cloud",
    description: "Deploy apps with flyctl.",
    cli: "flyctl",
  },
  {
    id: "hetzner",
    name: "Hetzner",
    category: "deploy_cloud",
    description: "Hetzner Cloud servers through hcloud.",
    cli: "hcloud",
  },
];

export const REMOTE_MCP_URL = "https://mcp.stackspirit.io/mcp";

export const MCP_CLIENTS: ReadonlyArray<{ id: McpClientId; label: string }> = [
  { id: "claude-desktop", label: "Claude Desktop" },
  { id: "claude-code", label: "Claude Code" },
  { id: "antigravity", label: "Antigravity Desktop" },
  { id: "cursor", label: "Cursor" },
  { id: "opencode", label: "OpenCode" },
];

/**
 * The config a client needs to reach StackSpirit. Local is the account-free stdio
 * `stackspirit mcp`; Remote is the account OAuth endpoint. Neither contains a token:
 * the remote client runs its own OAuth sign-in.
 */
export function clientConfig(client: McpClientId, mode: McpMode): string {
  const local = mode === "local";
  switch (client) {
    case "claude-code":
      return local
        ? "claude mcp add stackspirit -- stackspirit mcp"
        : `claude mcp add --transport http stackspirit ${REMOTE_MCP_URL}`;
    case "opencode":
      return JSON.stringify(
        {
          mcp: {
            stackspirit: local
              ? { type: "local", command: ["stackspirit", "mcp"] }
              : { type: "remote", url: REMOTE_MCP_URL },
          },
        },
        null,
        2,
      );
    case "antigravity":
      return JSON.stringify(
        {
          mcpServers: {
            stackspirit: local
              ? { command: "stackspirit", args: ["mcp"] }
              : { serverUrl: REMOTE_MCP_URL },
          },
        },
        null,
        2,
      );
    default:
      return JSON.stringify(
        {
          mcpServers: {
            stackspirit: local
              ? { command: "stackspirit", args: ["mcp"] }
              : client === "cursor"
                ? { url: REMOTE_MCP_URL }
                : {
                    command: "npx",
                    args: ["-y", "mcp-remote", REMOTE_MCP_URL],
                  },
          },
        },
        null,
        2,
      );
  }
}

/** Case-insensitive, all-words search plus an optional category filter. */
export function filterTools(
  tools: ReadonlyArray<ToolCatalogEntry>,
  query: string,
  category: ToolCategoryId | null,
): ToolCatalogEntry[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  return tools.filter((tool) => {
    if (category && tool.category !== category) return false;
    const hay =
      `${tool.id} ${tool.name} ${tool.description} ${tool.cli ?? ""}`.toLowerCase();
    return words.every((word) => hay.includes(word));
  });
}

const SECRET_RE =
  /(token=|key=|secret|password|bearer |authorization|apikey|sk-|\/\/[^/\s]*@)/i;

/** Returns an error message, or null when the draft is acceptable. */
export function validateDraft(draft: CustomToolDraft): string | null {
  if (!draft.name.trim()) return "Give the tool a name.";
  if (draft.kind === "cli") {
    if (!draft.command.trim() || /[\s/]/.test(draft.command.trim())) {
      return "Enter a bare executable name, such as mytool.";
    }
  } else if (draft.transport === "stdio") {
    if (!draft.command.trim())
      return "Enter the command that starts the MCP server.";
  } else if (!/^https?:\/\/[^\s@/]+(\/\S*)?$/.test(draft.url.trim())) {
    return "Enter an http(s) URL with no embedded credentials.";
  }
  if (SECRET_RE.test(`${draft.command} ${draft.url}`)) {
    return "Do not paste a secret here. Keep it in the secret vault.";
  }
  return null;
}

export interface CliMcpConnectPanelProps {
  homeConnected: boolean;
  homeLabel?: string;
  /** Extra entries, such as saved custom tools from spirit-core. */
  customTools?: ReadonlyArray<ToolCatalogEntry>;
  /** Installs StackSpirit's MCP entry into a client. Omitted when the host cannot. */
  onInstallClient?: (
    client: McpClientId,
    mode: McpMode,
  ) => Promise<void> | void;
  /** Connects a tool on the chosen target. Resolves with the per-side result. */
  onConnectTool?: (
    id: string,
    target: ToolTargetChoice,
  ) => Promise<Partial<ToolSideStatus>>;
  /** Saves a custom entry. Omitted when the host cannot persist it. */
  onSaveCustom?: (
    draft: CustomToolDraft,
  ) => Promise<ToolCatalogEntry> | ToolCatalogEntry;
}

const DOT_LABEL: Record<ToolSideState, string> = {
  not_connected: "not connected",
  connecting: "connecting",
  connected: "connected",
  needs_attention: "needs attention",
};

const EMPTY_DRAFT: CustomToolDraft = {
  kind: "cli",
  name: "",
  command: "",
  transport: "stdio",
  url: "",
};

/** Settings → CLI & MCP Connect: external-app install plus a searchable tools hub. */
export function CliMcpConnectPanel({
  homeConnected,
  homeLabel = "Always-On Home",
  customTools = [],
  onInstallClient,
  onConnectTool,
  onSaveCustom,
}: CliMcpConnectPanelProps) {
  const [mode, setMode] = useState<McpMode>("local");
  const [copied, setCopied] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<ToolCategoryId | null>(null);
  const [targets, setTargets] = useState<Record<string, ToolTargetChoice>>({});
  const [statuses, setStatuses] = useState<Record<string, ToolSideStatus>>({});
  const [added, setAdded] = useState<ToolCatalogEntry[]>([]);
  const [formOpen, setFormOpen] = useState(false);
  const [draft, setDraft] = useState<CustomToolDraft>(EMPTY_DRAFT);
  const [formError, setFormError] = useState<string | null>(null);

  const all = useMemo(
    () => [...BUILTIN_TOOLS, ...customTools, ...added],
    [customTools, added],
  );
  const visible = useMemo(
    () => filterTools(all, query, category),
    [all, query, category],
  );

  async function copy(text: string, label: string) {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(label);
      setMessage(null);
    } catch {
      setMessage("This window cannot copy to the clipboard.");
    }
  }

  async function install(client: McpClientId, label: string) {
    if (!onInstallClient) return;
    try {
      await onInstallClient(client, mode);
      setMessage(`Installed StackSpirit MCP into ${label}.`);
    } catch (caught) {
      setMessage(
        caught instanceof Error
          ? caught.message
          : `Could not install into ${label}.`,
      );
    }
  }

  function targetOf(id: string): ToolTargetChoice {
    const chosen = targets[id] ?? "both";
    return !homeConnected && chosen !== "local" ? "local" : chosen;
  }

  async function connect(tool: ToolCatalogEntry) {
    if (!onConnectTool) return;
    const target = targetOf(tool.id);
    const prior = statuses[tool.id] ?? {
      mac: "not_connected",
      home: "not_connected",
    };
    setStatuses((s) => ({
      ...s,
      [tool.id]: {
        mac: target !== "home" ? "connecting" : prior.mac,
        home: target !== "local" ? "connecting" : prior.home,
      },
    }));
    try {
      const result = await onConnectTool(tool.id, target);
      setStatuses((s) => ({ ...s, [tool.id]: { ...prior, ...result } }));
    } catch {
      setStatuses((s) => ({
        ...s,
        [tool.id]: {
          mac: target !== "home" ? "needs_attention" : prior.mac,
          home: target !== "local" ? "needs_attention" : prior.home,
        },
      }));
    }
  }

  async function saveCustom() {
    const problem = validateDraft(draft);
    if (problem) return setFormError(problem);
    if (!onSaveCustom)
      return setFormError("Saving custom tools needs the desktop app.");
    try {
      const saved = await onSaveCustom(draft);
      setAdded((list) => [...list, saved]);
      setDraft(EMPTY_DRAFT);
      setFormOpen(false);
      setFormError(null);
    } catch (caught) {
      setFormError(
        caught instanceof Error ? caught.message : "Could not save the tool.",
      );
    }
  }

  return (
    <div className="nx-settings-section" data-testid="cli-mcp-panel">
      <header className="nx-settings-section__head">
        <h3>CLI &amp; MCP Connect</h3>
      </header>

      <section className="nx-settings__card" aria-labelledby="ext-apps-heading">
        <h4 id="ext-apps-heading">
          Control StackSpirit from external apps (StackSpirit MCP)
        </h4>
        <div
          className="nx-cli-targets"
          role="group"
          aria-label="StackSpirit MCP mode"
        >
          <button
            type="button"
            className="nx-cli-target"
            aria-pressed={mode === "local"}
            onClick={() => setMode("local")}
            data-testid="mcp-mode-local"
          >
            Local (account-free)
            <small>
              <code>stackspirit mcp</code> over stdio
            </small>
          </button>
          <button
            type="button"
            className="nx-cli-target"
            aria-pressed={mode === "remote"}
            onClick={() => setMode("remote")}
            data-testid="mcp-mode-remote"
          >
            Remote (Account OAuth)
            <small>
              <code>{REMOTE_MCP_URL}</code>
            </small>
          </button>
        </div>
        <ul className="nx-cli-tools" aria-label="External apps">
          {MCP_CLIENTS.map((client) => (
            <li
              key={client.id}
              className="nx-cli-tool"
              data-testid={`mcp-client-${client.id}`}
            >
              <strong>{client.label}</strong>
              <button
                type="button"
                className="nx-btn nx-btn--sm"
                disabled={!onInstallClient}
                title={
                  onInstallClient
                    ? undefined
                    : "One-click install needs the desktop app. Copy the config instead."
                }
                aria-label={`Install into ${client.label}`}
                onClick={() => void install(client.id, client.label)}
              >
                Install
              </button>
              <button
                type="button"
                className="nx-btn nx-btn--quiet nx-btn--sm"
                aria-label={`Copy config for ${client.label}`}
                onClick={() =>
                  void copy(clientConfig(client.id, mode), client.label)
                }
              >
                {copied === client.label ? "Copied" : "Copy config"}
              </button>
            </li>
          ))}
        </ul>
        <p className="nx-form__hint">
          No token is in these configs. Remote clients sign in with their own
          OAuth flow.
        </p>
      </section>

      <section
        className="nx-settings__card"
        aria-labelledby="tools-hub-heading"
      >
        <h4 id="tools-hub-heading">CLI &amp; MCP tools</h4>
        <input
          type="search"
          className="nx-input"
          placeholder="Search tools"
          aria-label="Search tools"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
        />
        <div role="group" aria-label="Categories" className="nx-cli-targets">
          <button
            type="button"
            className="nx-chip"
            aria-pressed={category === null}
            onClick={() => setCategory(null)}
          >
            All
          </button>
          {CATEGORIES.map((item) => (
            <button
              key={item.id}
              type="button"
              className="nx-chip"
              aria-pressed={category === item.id}
              onClick={() => setCategory(category === item.id ? null : item.id)}
              data-testid={`category-${item.id}`}
            >
              {item.label}
            </button>
          ))}
        </div>

        <ul
          className="nx-cli-tools"
          aria-label="Tools"
          style={{ maxHeight: 420, overflowY: "auto" }}
        >
          {visible.map((tool) => {
            const status = statuses[tool.id] ?? {
              mac: "not_connected",
              home: "not_connected",
            };
            const target = targetOf(tool.id);
            return (
              <li
                key={tool.id}
                className="nx-cli-tool"
                data-testid={`tool-${tool.id}`}
              >
                <div className="nx-cli-tool__name">
                  <strong>{tool.name}</strong>
                  <span className="nx-provider__meta">
                    {[
                      tool.cli ? `CLI ${tool.cli}` : null,
                      tool.mcp ? `MCP ${tool.mcp}` : null,
                    ]
                      .filter(Boolean)
                      .join(" · ")}{" "}
                    · {tool.description}
                  </span>
                </div>
                <select
                  aria-label={`Target for ${tool.name}`}
                  value={target}
                  onChange={(event) =>
                    setTargets((t) => ({
                      ...t,
                      [tool.id]: event.target.value as ToolTargetChoice,
                    }))
                  }
                >
                  <option value="both" disabled={!homeConnected}>
                    Both (Recommended)
                  </option>
                  <option value="local">Local Mac</option>
                  <option value="home" disabled={!homeConnected}>
                    {homeLabel}
                  </option>
                </select>
                <span
                  className={`nx-dot nx-dot--${status.mac}`}
                  role="img"
                  aria-label={`Mac ${DOT_LABEL[status.mac]}`}
                  data-testid={`dot-mac-${tool.id}`}
                />
                <span
                  className={`nx-dot nx-dot--${status.home}`}
                  role="img"
                  aria-label={`Home ${DOT_LABEL[status.home]}`}
                  data-testid={`dot-home-${tool.id}`}
                />
                <button
                  type="button"
                  className="nx-btn nx-btn--sm"
                  disabled={
                    !onConnectTool ||
                    status.mac === "connecting" ||
                    status.home === "connecting"
                  }
                  title={
                    onConnectTool ? undefined : "Connect needs the desktop app."
                  }
                  aria-label={`Connect ${tool.name}`}
                  onClick={() => void connect(tool)}
                >
                  Connect
                </button>
              </li>
            );
          })}
        </ul>
        {visible.length === 0 ? (
          <p className="nx-form__hint" role="status">
            No tools match.
          </p>
        ) : null}
        {!homeConnected ? (
          <p className="nx-form__hint">
            No Always-On Home is connected, so tools connect on this Mac only.
          </p>
        ) : null}

        <button
          type="button"
          className="nx-btn nx-btn--quiet"
          onClick={() => setFormOpen((open) => !open)}
          aria-expanded={formOpen}
        >
          + Add custom CLI / MCP
        </button>
        {formOpen ? (
          <form
            data-testid="custom-tool-form"
            onSubmit={(event) => {
              event.preventDefault();
              void saveCustom();
            }}
          >
            <select
              aria-label="Custom tool kind"
              value={draft.kind}
              onChange={(e) =>
                setDraft({ ...draft, kind: e.target.value as "cli" | "mcp" })
              }
            >
              <option value="cli">Custom CLI</option>
              <option value="mcp">Custom MCP</option>
            </select>
            <input
              aria-label="Custom tool name"
              placeholder="Name"
              value={draft.name}
              onChange={(e) => setDraft({ ...draft, name: e.target.value })}
            />
            {draft.kind === "mcp" ? (
              <select
                aria-label="MCP transport"
                value={draft.transport}
                onChange={(e) =>
                  setDraft({
                    ...draft,
                    transport: e.target.value as CustomToolDraft["transport"],
                  })
                }
              >
                <option value="stdio">stdio</option>
                <option value="http">HTTP</option>
                <option value="sse">SSE</option>
              </select>
            ) : null}
            {draft.kind === "cli" || draft.transport === "stdio" ? (
              <input
                aria-label="Command"
                placeholder={
                  draft.kind === "cli"
                    ? "Binary, e.g. mytool"
                    : "Command, e.g. npx"
                }
                value={draft.command}
                onChange={(e) =>
                  setDraft({ ...draft, command: e.target.value })
                }
              />
            ) : (
              <input
                aria-label="URL"
                placeholder="https://…"
                value={draft.url}
                onChange={(e) => setDraft({ ...draft, url: e.target.value })}
              />
            )}
            <button type="submit" className="nx-btn nx-btn--sm">
              Save tool
            </button>
            <p className="nx-form__hint">
              Never paste a key or token. Secrets live in the secret vault.
            </p>
            {formError ? (
              <p className="nx-form__error" role="alert">
                {formError}
              </p>
            ) : null}
          </form>
        ) : null}
      </section>

      {message ? (
        <p className="nx-form__hint" role="status">
          {message}
        </p>
      ) : null}
    </div>
  );
}
