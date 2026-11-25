import { Fragment, useCallback, useEffect, useId, useMemo, useRef, useState, type FormEvent, type ReactNode } from "react";

import { Modal } from "../components";
import { toSpiritError } from "../bridge/errors";
import type { SpiritBridge } from "../bridge";
import type { RemoteDirListing, RemoteEntry, RemotePreview } from "../bridge/wire";
import { formatBytes, formatDateTime } from "../domain/format";
import { useFleet, useFleetStore } from "../state/fleetStore";
import { Menu, MenuItem } from "./Menu";
import { PageHeader } from "./PageHeader";

/** The exact seatbelt warning shown before touching a live VOWP path. */
export const SEATBELT_TEXT =
  "This is a live StackSpirit system path — moving or deleting it may interrupt running work. Continue anyway?";
/** The exact placeholder shown for a secret-like file until it is revealed locally. */
export const MASKED_TEXT = "Secret file hidden by default — never sent to AI";

type Files = NonNullable<SpiritBridge["files"]>;

/** What the owner asked for; kept so the seatbelt can run it after confirmation. */
type Action =
  | { kind: "move"; entry: RemoteEntry; to: string; label: string }
  | { kind: "delete"; entry: RemoteEntry };

type Dialog =
  | { kind: "mkdir" }
  | { kind: "rename"; entry: RemoteEntry }
  | { kind: "move"; entry: RemoteEntry }
  | { kind: "delete"; entry: RemoteEntry }
  | { kind: "seatbelt"; action: Action };

/** Subtle explanations for the VOWP home, shown next to the names they describe. */
const VOWP_HINTS: Record<string, string> = {
  "vowp.db": "live database",
  "vowp.toml": "config",
  "stackspirit.toml": "config",
  logs: "supervisor logs",
  backups: "backups",
  worktrees: "worker checkouts",
};

const VOWP_LOCATION_BADGES: { name: string; text: string }[] = [
  { name: "vowp.db", text: "live database" },
  { name: "vowp.toml", text: "config" },
  { name: "logs/", text: "supervisor logs" },
  { name: "worktrees/", text: "worker checkouts" },
];

function joinPath(dir: string, name: string): string {
  return dir.endsWith("/") ? `${dir}${name}` : `${dir}/${name}`;
}

function parentOf(path: string): string {
  const trimmed = path.replace(/\/+$/, "");
  const index = trimmed.lastIndexOf("/");
  return index <= 0 ? "/" : trimmed.slice(0, index);
}

/** Mirror of the core rule, used only to show the seatbelt before a round trip. */
function looksProtected(path: string): boolean {
  const parts = path.split("/").filter((part) => part && part !== "~");
  return parts.some(
    (part, index) =>
      part.startsWith("vowp.db") || part === "vowp.toml" || part === "stackspirit.toml" || part === ".ssh" || (part === "worktrees" && index + 1 < parts.length),
  );
}

function isProtected(entry: RemoteEntry): boolean {
  return entry.is_protected_hint || looksProtected(entry.path);
}

/** Breadcrumb segments; the home folder is shown as one `Home` crumb. */
function crumbs(listing: RemoteDirListing): { label: string; path: string }[] {
  const { path, home } = listing;
  const out: { label: string; path: string }[] = [];
  let rest: string[];
  if (home && (path === home || path.startsWith(`${home}/`))) {
    out.push({ label: "Home", path: home });
    rest = path.slice(home.length).split("/").filter(Boolean);
    let current = home;
    for (const part of rest) {
      current = joinPath(current, part);
      out.push({ label: part, path: current });
    }
    return out;
  }
  out.push({ label: "/", path: "/" });
  let current = "";
  for (const part of path.split("/").filter(Boolean)) {
    current = `${current}/${part}`;
    out.push({ label: part, path: current });
  }
  return out;
}

function errorMessage(error: unknown): string {
  const normalized = toSpiritError(error);
  return normalized.hint ? `${normalized.message} ${normalized.hint}` : normalized.message;
}

function isProtectedError(error: unknown): boolean {
  return toSpiritError(error).message.startsWith("protected_path");
}

function FolderIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 18 18" aria-hidden="true">
      <path d="M2 5.5A1.5 1.5 0 0 1 3.5 4h3l1.5 1.5h6A1.5 1.5 0 0 1 15.5 7v5.5A1.5 1.5 0 0 1 14 14H3.5A1.5 1.5 0 0 1 2 12.5z" fill="currentColor" opacity="0.85" />
    </svg>
  );
}

function FileIcon({ link = false }: { link?: boolean }) {
  return (
    <svg width="18" height="18" viewBox="0 0 18 18" aria-hidden="true">
      <path d="M4.5 2.5h6l3 3v9a1 1 0 0 1-1 1h-8a1 1 0 0 1-1-1v-11a1 1 0 0 1 1-1z" fill="none" stroke="currentColor" strokeWidth="1.4" />
      {link ? <path d="M7 11l4-4M8 7h3v3" fill="none" stroke="currentColor" strokeWidth="1.4" /> : <path d="M6.5 9h5M6.5 11.5h5" stroke="currentColor" strokeWidth="1.2" />}
    </svg>
  );
}

function MoreIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" aria-hidden="true">
      <circle cx="3.5" cy="8" r="1.3" fill="currentColor" />
      <circle cx="8" cy="8" r="1.3" fill="currentColor" />
      <circle cx="12.5" cy="8" r="1.3" fill="currentColor" />
    </svg>
  );
}

/** Inline emphasis and code spans; everything else stays plain text (no HTML ever). */
function inline(text: string): ReactNode[] {
  const nodes: ReactNode[] = [];
  const pattern = /(`[^`]+`|\*\*[^*]+\*\*)/g;
  let last = 0;
  for (const match of text.matchAll(pattern)) {
    const index = match.index ?? 0;
    if (index > last) nodes.push(text.slice(last, index));
    const token = match[0];
    nodes.push(token.startsWith("`") ? <code key={index}>{token.slice(1, -1)}</code> : <strong key={index}>{token.slice(2, -2)}</strong>);
    last = index + token.length;
  }
  if (last < text.length) nodes.push(text.slice(last));
  return nodes;
}

/** A deliberately small, safe Markdown view: headings, bullets, code fences, paragraphs. */
export function MarkdownPreview({ source }: { source: string }) {
  const blocks: ReactNode[] = [];
  const lines = source.split("\n");
  let index = 0;
  let key = 0;
  while (index < lines.length) {
    const line = lines[index] ?? "";
    if (line.startsWith("```")) {
      const code: string[] = [];
      index += 1;
      while (index < lines.length && !(lines[index] ?? "").startsWith("```")) {
        code.push(lines[index] ?? "");
        index += 1;
      }
      index += 1;
      blocks.push(
        <pre key={key++} className="nx-files__code">
          {code.join("\n")}
        </pre>,
      );
    } else if (/^#{1,6}\s/.test(line)) {
      const level = Math.min(6, (line.match(/^#+/)?.[0].length ?? 1) + 2);
      const Tag = `h${level}` as "h3";
      blocks.push(<Tag key={key++}>{inline(line.replace(/^#+\s*/, ""))}</Tag>);
      index += 1;
    } else if (/^\s*[-*]\s+/.test(line)) {
      const items: ReactNode[] = [];
      while (index < lines.length && /^\s*[-*]\s+/.test(lines[index] ?? "")) {
        items.push(<li key={items.length}>{inline((lines[index] ?? "").replace(/^\s*[-*]\s+/, ""))}</li>);
        index += 1;
      }
      blocks.push(<ul key={key++}>{items}</ul>);
    } else if (line.trim() === "") {
      index += 1;
    } else {
      const para: string[] = [];
      while (index < lines.length && (lines[index] ?? "").trim() !== "" && !/^(#{1,6}\s|```|\s*[-*]\s+)/.test(lines[index] ?? "")) {
        para.push(lines[index] ?? "");
        index += 1;
      }
      blocks.push(<p key={key++}>{inline(para.join(" "))}</p>);
    }
  }
  return <div className="nx-files__md">{blocks}</div>;
}

interface Location {
  id: string;
  label: string;
  path: string;
  note?: string;
}

/**
 * A Finder-style file manager for the connected server, in the Meadow theme.
 *
 * Left: quick locations (Home, one per project, the VOWP home with plain-language
 * badges, Root). Middle: breadcrumb, filter, toolbar and a table whose rows can be dragged
 * onto folders. Right: a bounded preview. Secret-like files stay masked until the local
 * user reveals them, and a live VOWP path asks for an explicit confirmation first. The
 * contents of a file never go to the AI.
 */
export function RemoteFilesView({ initialPath = "~", onBack }: { initialPath?: string; onBack?: () => void }) {
  const bridge = useFleetStore((state) => state.bridge);
  const fleet = useFleet();
  const files = bridge?.files;
  if (!files) {
    return (
      <section className="nx-main" aria-label="Files">
        <PageHeader title="Files" sub="Browse the server" />
        <div className="nx-page">
          <div className="nx-page__inner">
            <p className="nx-rows__empty">File browsing needs the desktop app.</p>
          </div>
        </div>
      </section>
    );
  }
  return <FilesBrowser files={files} projects={(fleet?.projects ?? []).map((project) => project.name)} initialPath={initialPath} onBack={onBack} />;
}

function FilesBrowser({ files, projects, initialPath, onBack }: { files: Files; projects: string[]; initialPath: string; onBack?: () => void }) {
  const [listing, setListing] = useState<RemoteDirListing | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<RemoteEntry | null>(null);
  const [loaded, setLoaded] = useState<{ key: string; preview: RemotePreview | null; error: string | null } | null>(null);
  const [reveal, setReveal] = useState(false);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [busy, setBusy] = useState(false);
  const [dragging, setDragging] = useState<RemoteEntry | null>(null);
  const [dropTarget, setDropTarget] = useState<string | null>(null);
  const loadSeq = useRef(0);
  const filterId = useId();

  const apply = useCallback((next: RemoteDirListing) => {
    setListing(next);
    setSelected(null);
    setLoaded(null);
    setReveal(false);
    setFilter("");
  }, []);

  /** Opens a folder from an event handler (a click); the first load runs in the effect. */
  const load = useCallback(
    async (path: string) => {
      const seq = ++loadSeq.current;
      setLoading(true);
      setError(null);
      try {
        const next = await files.list(path);
        if (seq === loadSeq.current) apply(next);
      } catch (failure) {
        if (seq === loadSeq.current) setError(errorMessage(failure));
      } finally {
        if (seq === loadSeq.current) setLoading(false);
      }
    },
    [files, apply],
  );

  useEffect(() => {
    const seq = ++loadSeq.current;
    files
      .list(initialPath)
      .then((next) => {
        if (seq === loadSeq.current) apply(next);
      })
      .catch((failure: unknown) => {
        if (seq === loadSeq.current) setError(errorMessage(failure));
      })
      .finally(() => {
        if (seq === loadSeq.current) setLoading(false);
      });
  }, [files, initialPath, apply]);

  // Read the preview of the selected file. A masked file is never read by the host.
  // The result is keyed by file and reveal state, so a stale answer is never shown.
  const previewKey = selected && selected.kind !== "dir" ? `${selected.path}|${reveal ? "1" : "0"}` : null;
  useEffect(() => {
    if (!previewKey || !selected) return;
    let cancelled = false;
    files
      .preview(selected.path, reveal)
      .then((next) => {
        if (!cancelled) setLoaded({ key: previewKey, preview: next, error: null });
      })
      .catch((failure: unknown) => {
        if (!cancelled) setLoaded({ key: previewKey, preview: null, error: errorMessage(failure) });
      });
    return () => {
      cancelled = true;
    };
    // `selected` and `reveal` are folded into `previewKey`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [files, previewKey]);
  const current = loaded && loaded.key === previewKey ? loaded : null;
  const preview = current?.preview ?? null;
  const previewError = current?.error ?? null;
  const previewBusy = previewKey !== null && current === null;

  const currentPath = listing?.path ?? initialPath;
  const refresh = () => void load(currentPath);

  const locations = useMemo(() => {
    const items: Location[] = [{ id: "home", label: "Home", path: "~" }];
    for (const name of projects) items.push({ id: `project:${name}`, label: name, path: `~/${name}` });
    return items;
  }, [projects]);

  const visible = useMemo(() => {
    const needle = filter.trim().toLowerCase();
    const entries = listing?.entries ?? [];
    return needle ? entries.filter((entry) => entry.name.toLowerCase().includes(needle)) : entries;
  }, [listing, filter]);

  /** Runs one action; a protected path first asks for the seatbelt confirmation. */
  async function execute(action: Action, confirmed: boolean) {
    if (!confirmed && isProtected(action.entry)) {
      setDialog({ kind: "seatbelt", action });
      return;
    }
    setBusy(true);
    setError(null);
    try {
      if (action.kind === "move") {
        await files.move({ from: action.entry.path, to: action.to, confirm_protected: confirmed });
        setNotice(`${action.label} ${action.entry.name}`);
      } else {
        await files.delete({ path: action.entry.path, recursive: action.entry.kind === "dir", confirm_protected: confirmed });
        setNotice(`Deleted ${action.entry.name}`);
      }
      setDialog(null);
      await load(currentPath);
    } catch (failure) {
      if (!confirmed && isProtectedError(failure)) {
        setDialog({ kind: "seatbelt", action });
      } else {
        setDialog(null);
        setError(errorMessage(failure));
      }
    } finally {
      setBusy(false);
    }
  }

  async function createFolder(name: string) {
    setBusy(true);
    setError(null);
    try {
      await files.mkdir(joinPath(currentPath, name));
      setNotice(`Created folder ${name}`);
      setDialog(null);
      await load(currentPath);
    } catch (failure) {
      setDialog(null);
      setError(errorMessage(failure));
    } finally {
      setBusy(false);
    }
  }

  async function upload() {
    setError(null);
    try {
      const outcome = await files.upload(currentPath);
      if (!outcome) return;
      setNotice(`Uploaded ${outcome.path.slice(outcome.path.lastIndexOf("/") + 1)}`);
      await load(currentPath);
    } catch (failure) {
      setError(errorMessage(failure));
    }
  }

  async function download(entry: RemoteEntry) {
    setError(null);
    try {
      const outcome = await files.download(entry.path);
      if (outcome) setNotice(`Saved ${entry.name} to ${outcome.local_path}`);
    } catch (failure) {
      setError(errorMessage(failure));
    }
  }

  async function copyPath(entry: RemoteEntry) {
    try {
      await navigator.clipboard.writeText(entry.path);
      setNotice(`Copied path of ${entry.name}`);
    } catch {
      setNotice(`Path: ${entry.path}`);
    }
  }

  function dropOnto(folder: RemoteEntry) {
    const source = dragging;
    setDragging(null);
    setDropTarget(null);
    if (!source || source.path === folder.path || folder.kind !== "dir") return;
    void execute({ kind: "move", entry: source, to: joinPath(folder.path, source.name), label: `Moved into ${folder.name}:` }, false);
  }

  const inVowpHome = listing ? /\/\.vowp(\/|$)/.test(listing.path) : false;
  const crumbList = listing ? crumbs(listing) : [];

  return (
    <section className="nx-main" aria-label="Files" data-testid="remote-files">
      <PageHeader
        title="Files"
        sub="Browse and manage this server"
        actions={
          onBack ? (
            <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={onBack}>
              Back to server
            </button>
          ) : undefined
        }
      />
      <div className="nx-files">
        <nav className="nx-files__side" aria-label="Locations">
          <h2 className="nx-files__side-title">Locations</h2>
          <ul>
            <li>
              <LocationButton active={currentPath === listing?.home} label="Home" path="~" onOpen={load} sub="~" />
            </li>
          </ul>
          <h2 className="nx-files__side-title">Projects</h2>
          {locations.length <= 1 ? (
            <p className="nx-files__side-empty">No projects yet.</p>
          ) : (
            <ul>
              {locations
                .filter((location) => location.id !== "home")
                .map((location) => (
                  <li key={location.id}>
                    <LocationButton active={listing?.path === `${listing?.home}/${location.label}`} label={location.label} path={location.path} onOpen={load} />
                  </li>
                ))}
            </ul>
          )}
          <h2 className="nx-files__side-title">StackSpirit</h2>
          <ul>
            <li>
              <LocationButton active={inVowpHome && listing?.path.endsWith("/.vowp")} label="StackSpirit Home" path="~/.vowp" onOpen={load} sub="~/.vowp" />
              <ul className="nx-files__badges" aria-label="What lives in the StackSpirit home">
                {VOWP_LOCATION_BADGES.map((badge) => (
                  <li key={badge.name}>
                    <code>{badge.name}</code> <span>{badge.text}</span>
                  </li>
                ))}
              </ul>
            </li>
          </ul>
          <h2 className="nx-files__side-title">Computer</h2>
          <ul>
            <li>
              <LocationButton active={listing?.path === "/"} label="Root" path="/" onOpen={load} sub="/" />
            </li>
          </ul>
        </nav>

        <div className="nx-files__main">
          <div className="nx-files__bar">
            <nav aria-label="Path" className="nx-files__crumbs">
              <ol>
                {crumbList.map((crumb, index) => (
                  <li key={crumb.path}>
                    {index > 0 ? <span aria-hidden="true">/</span> : null}
                    <button
                      type="button"
                      className="nx-files__crumb"
                      aria-current={index === crumbList.length - 1 ? "page" : undefined}
                      onClick={() => void load(crumb.path)}
                    >
                      {crumb.label}
                    </button>
                  </li>
                ))}
              </ol>
            </nav>
            <div className="nx-files__tools">
              <label className="nx-files__filter" htmlFor={filterId}>
                <span className="nx-sr">Filter files</span>
                <input id={filterId} type="search" placeholder="Filter" value={filter} onChange={(event) => setFilter(event.target.value)} autoComplete="off" spellCheck={false} />
              </label>
              <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={() => setDialog({ kind: "mkdir" })} disabled={!listing}>
                New folder
              </button>
              <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={() => void upload()} disabled={!listing}>
                Upload
              </button>
              <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={refresh}>
                Refresh
              </button>
            </div>
          </div>

          {error ? (
            <p className="nx-notice--error" role="alert" data-testid="files-error">
              {error}
            </p>
          ) : null}
          {notice ? (
            <p className="nx-notice--status" role="status" data-testid="files-notice">
              {notice}
            </p>
          ) : null}

          <div className="nx-files__scroll" aria-busy={loading}>
            {listing ? (
              <table className="nx-files__table">
                <caption className="nx-sr">Contents of {listing.path}</caption>
                <thead>
                  <tr>
                    <th scope="col">Name</th>
                    <th scope="col">Kind</th>
                    <th scope="col" className="nx-files__num">
                      Size
                    </th>
                    <th scope="col">Mode</th>
                    <th scope="col">Modified</th>
                    <th scope="col">
                      <span className="nx-sr">Actions</span>
                    </th>
                  </tr>
                </thead>
                <tbody>
                  {listing.parent ? (
                    <tr className="nx-files__row">
                      <td colSpan={6}>
                        <button type="button" className="nx-files__name" onClick={() => void load(listing.parent as string)}>
                          <FolderIcon />
                          <span>..</span>
                          <span className="nx-sr"> parent folder</span>
                        </button>
                      </td>
                    </tr>
                  ) : null}
                  {visible.map((entry) => {
                    const isDir = entry.kind === "dir";
                    const hint = inVowpHome ? VOWP_HINTS[entry.name] : undefined;
                    return (
                      <tr
                        key={entry.path}
                        className={`nx-files__row${selected?.path === entry.path ? " is-selected" : ""}${dropTarget === entry.path ? " is-drop" : ""}`}
                        data-testid={`file-row-${entry.name}`}
                        draggable
                        onDragStart={(event) => {
                          setDragging(entry);
                          event.dataTransfer?.setData("text/plain", entry.path);
                          if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
                        }}
                        onDragEnd={() => {
                          setDragging(null);
                          setDropTarget(null);
                        }}
                        onDragOver={(event) => {
                          if (isDir && dragging && dragging.path !== entry.path) {
                            event.preventDefault();
                            setDropTarget(entry.path);
                          }
                        }}
                        onDragLeave={() => setDropTarget((current) => (current === entry.path ? null : current))}
                        onDrop={(event) => {
                          if (!isDir) return;
                          event.preventDefault();
                          dropOnto(entry);
                        }}
                      >
                        <td>
                          <button
                            type="button"
                            className="nx-files__name"
                            aria-pressed={!isDir ? selected?.path === entry.path : undefined}
                            onClick={() => (isDir ? void load(entry.path) : setSelected(entry))}
                          >
                            {isDir ? <FolderIcon /> : <FileIcon link={entry.kind === "symlink"} />}
                            <span>{entry.name}</span>
                          </button>
                          {hint ? <span className="nx-files__hint">{hint}</span> : null}
                        </td>
                        <td>
                          <span className="nx-files__kind">{isDir ? "Folder" : entry.kind === "symlink" ? "Link" : "File"}</span>
                          {entry.is_secret_like ? <span className="nx-files__chip nx-files__chip--secret">Secret</span> : null}
                          {entry.is_protected_hint ? <span className="nx-files__chip nx-files__chip--live">Live StackSpirit path</span> : null}
                        </td>
                        <td className="nx-files__num">{isDir ? "—" : formatBytes(entry.size_bytes)}</td>
                        <td>
                          <code>{entry.mode}</code>
                        </td>
                        <td>{entry.modified_at ? formatDateTime(entry.modified_at) : "—"}</td>
                        <td className="nx-files__actions">
                          <Menu
                            label={`Actions for ${entry.name}`}
                            align="end"
                            trigger={(props) => (
                              <button type="button" className="nx-icon-btn nx-icon-btn--sm" aria-label={`Actions for ${entry.name}`} {...props}>
                                <MoreIcon />
                              </button>
                            )}
                          >
                            {(close) => (
                              <Fragment>
                                <MenuItem
                                  onSelect={() => {
                                    close();
                                    setDialog({ kind: "move", entry });
                                  }}
                                >
                                  Move…
                                </MenuItem>
                                <MenuItem
                                  onSelect={() => {
                                    close();
                                    setDialog({ kind: "rename", entry });
                                  }}
                                >
                                  Rename
                                </MenuItem>
                                {!isDir ? (
                                  <MenuItem
                                    onSelect={() => {
                                      close();
                                      void download(entry);
                                    }}
                                  >
                                    Download
                                  </MenuItem>
                                ) : null}
                                <MenuItem
                                  onSelect={() => {
                                    close();
                                    void copyPath(entry);
                                  }}
                                >
                                  Copy path
                                </MenuItem>
                                <MenuItem
                                  onSelect={() => {
                                    close();
                                    setDialog({ kind: "delete", entry });
                                  }}
                                >
                                  Delete
                                </MenuItem>
                              </Fragment>
                            )}
                          </Menu>
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            ) : loading ? (
              <p className="nx-rows__empty">Loading…</p>
            ) : null}
            {listing && visible.length === 0 ? (
              <p className="nx-rows__empty">{filter ? "Nothing matches that filter." : "This folder is empty."}</p>
            ) : null}
            {listing?.truncated ? <p className="nx-rows__empty">Only the first entries are shown. Use the filter to narrow the folder.</p> : null}
          </div>
        </div>

        <aside className="nx-files__preview" aria-label="Preview">
          <PreviewPane
            entry={selected}
            preview={preview}
            busy={previewBusy}
            error={previewError}
            reveal={reveal}
            onToggleReveal={() => setReveal((value) => !value)}
          />
        </aside>
      </div>

      {dialog?.kind === "mkdir" ? (
        <NameDialog title="New folder" label="Folder name" confirm="Create" initial="" busy={busy} onClose={() => setDialog(null)} onSubmit={(name) => void createFolder(name)} />
      ) : null}
      {dialog?.kind === "rename" ? (
        <NameDialog
          title={`Rename ${dialog.entry.name}`}
          label="New name"
          confirm="Rename"
          initial={dialog.entry.name}
          busy={busy}
          onClose={() => setDialog(null)}
          onSubmit={(name) =>
            void execute({ kind: "move", entry: dialog.entry, to: joinPath(parentOf(dialog.entry.path), name), label: "Renamed" }, false)
          }
        />
      ) : null}
      {dialog?.kind === "move" ? (
        <NameDialog
          title={`Move ${dialog.entry.name}`}
          label="Destination folder"
          confirm="Move"
          initial={parentOf(dialog.entry.path)}
          busy={busy}
          validate={(value) => (value.startsWith("/") || value === "~" || value.startsWith("~/") ? null : "Use an absolute path or one starting with ~/.")}
          onClose={() => setDialog(null)}
          onSubmit={(destination) =>
            void execute({ kind: "move", entry: dialog.entry, to: joinPath(destination, dialog.entry.name), label: "Moved" }, false)
          }
        />
      ) : null}
      {dialog?.kind === "delete" ? (
        <Modal
          open
          title={`Delete ${dialog.entry.name}?`}
          danger
          onClose={() => setDialog(null)}
          footer={
            <>
              <button type="button" className="nx-btn nx-btn--quiet" onClick={() => setDialog(null)} disabled={busy}>
                Cancel
              </button>
              <button type="button" className="nx-btn" onClick={() => void execute({ kind: "delete", entry: dialog.entry }, false)} disabled={busy}>
                Delete
              </button>
            </>
          }
        >
          <p>
            {dialog.entry.kind === "dir"
              ? "This deletes the folder and everything inside it from the server. It cannot be undone."
              : "This deletes the file from the server. It cannot be undone."}
          </p>
        </Modal>
      ) : null}
      {dialog?.kind === "seatbelt" ? (
        <Modal
          open
          danger
          title="Live StackSpirit path"
          onClose={() => setDialog(null)}
          footer={
            <>
              <button type="button" className="nx-btn nx-btn--quiet" onClick={() => setDialog(null)} disabled={busy}>
                Cancel
              </button>
              <button type="button" className="nx-btn" onClick={() => void execute(dialog.action, true)} disabled={busy}>
                Continue anyway
              </button>
            </>
          }
        >
          <p data-testid="seatbelt-text">{SEATBELT_TEXT}</p>
          <p className="nx-form__hint">
            <code>{dialog.action.entry.path}</code>
          </p>
        </Modal>
      ) : null}
    </section>
  );
}

function LocationButton({ label, path, sub, active, onOpen }: { label: string; path: string; sub?: string; active?: boolean; onOpen: (path: string) => void }) {
  return (
    <button type="button" className={`nx-files__loc${active ? " is-active" : ""}`} aria-current={active ? "page" : undefined} onClick={() => void onOpen(path)}>
      <FolderIcon />
      <span className="nx-files__loc-text">
        <span>{label}</span>
        {sub ? <span className="nx-files__loc-sub">{sub}</span> : null}
      </span>
    </button>
  );
}

function NameDialog({
  title,
  label,
  confirm,
  initial,
  busy,
  validate,
  onClose,
  onSubmit,
}: {
  title: string;
  label: string;
  confirm: string;
  initial: string;
  busy: boolean;
  validate?: (value: string) => string | null;
  onClose: () => void;
  onSubmit: (value: string) => void;
}) {
  const [value, setValue] = useState(initial);
  const id = useId();
  const trimmed = value.trim();
  const custom = trimmed && validate ? validate(trimmed) : null;
  const nameError = !validate && trimmed && (trimmed.includes("/") || trimmed === "." || trimmed === "..") ? "A name cannot contain / or be . or .." : null;
  const problem = custom ?? nameError;
  const ready = Boolean(trimmed) && !problem && trimmed !== initial;
  const submit = (event?: FormEvent) => {
    event?.preventDefault();
    if (ready && !busy) onSubmit(trimmed);
  };
  return (
    <Modal
      open
      title={title}
      onClose={onClose}
      footer={
        <>
          <button type="button" className="nx-btn nx-btn--quiet" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="button" className="nx-btn" onClick={() => submit()} disabled={!ready || busy}>
            {confirm}
          </button>
        </>
      }
    >
      <form className="nx-form" onSubmit={submit}>
        <label className="nx-field" htmlFor={id}>
          {label}
          <input id={id} value={value} onChange={(event) => setValue(event.target.value)} autoComplete="off" spellCheck={false} aria-invalid={problem ? true : undefined} autoFocus />
        </label>
        {problem ? (
          <p className="nx-form__hint" role="alert">
            {problem}
          </p>
        ) : null}
      </form>
    </Modal>
  );
}

function PreviewPane({
  entry,
  preview,
  busy,
  error,
  reveal,
  onToggleReveal,
}: {
  entry: RemoteEntry | null;
  preview: RemotePreview | null;
  busy: boolean;
  error: string | null;
  reveal: boolean;
  onToggleReveal: () => void;
}) {
  if (!entry) {
    return <p className="nx-rows__empty">Select a file to preview it.</p>;
  }
  const masked = entry.is_secret_like && !reveal;
  return (
    <div className="nx-files__preview-body" data-testid="file-preview">
      <h2 className="nx-files__preview-title">{entry.name}</h2>
      <dl className="nx-files__meta">
        <dt>Path</dt>
        <dd>
          <code>{entry.path}</code>
        </dd>
        <dt>Size</dt>
        <dd>{formatBytes(entry.size_bytes)}</dd>
        <dt>Mode</dt>
        <dd>
          <code>{entry.mode}</code>
        </dd>
        <dt>Modified</dt>
        <dd>{entry.modified_at ? formatDateTime(entry.modified_at) : "—"}</dd>
      </dl>
      {entry.is_secret_like ? (
        <div className="nx-files__secret">
          {masked ? <p data-testid="masked-preview">{MASKED_TEXT}</p> : <p className="nx-form__hint">Revealed on this screen only. It is never sent to the AI.</p>}
          <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" aria-pressed={reveal} onClick={onToggleReveal}>
            {reveal ? "Hide" : "Reveal"}
          </button>
        </div>
      ) : null}
      {error ? (
        <p className="nx-notice--error" role="alert">
          {error}
        </p>
      ) : null}
      {masked ? null : busy && !preview ? (
        <p className="nx-rows__empty">Loading preview…</p>
      ) : preview && preview.path === entry.path && !preview.masked ? (
        <PreviewBody preview={preview} />
      ) : null}
    </div>
  );
}

function PreviewBody({ preview }: { preview: RemotePreview }) {
  if (preview.kind === "image" && preview.image_base64 && preview.image_mime) {
    return <img className="nx-files__image" alt={`Preview of ${preview.name}`} src={`data:${preview.image_mime};base64,${preview.image_base64}`} />;
  }
  if (preview.kind === "markdown" && preview.text !== null) {
    return <MarkdownPreview source={preview.text} />;
  }
  if (preview.kind === "text" && preview.text !== null) {
    return (
      <pre className="nx-files__code" tabIndex={0} aria-label={`Contents of ${preview.name}`}>
        {preview.text}
      </pre>
    );
  }
  return <p className="nx-rows__empty">{preview.note ?? "No preview for this file."}</p>;
}
