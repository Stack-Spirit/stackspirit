import { useId, useState, type FormEvent } from "react";

import { Modal } from "../components";
import type { ProjectAddInput } from "../bridge/wire";

export interface AddProjectDialogProps {
  serverLabel: string;
  existing: string[];
  /** The connected local node (display-only), or null when no local agent is connected. */
  localNode: string | null;
  onClose: () => void;
  /** Resolves on success (the dialog closes) and rejects with the error to show inline. */
  onCreate: (
    request: ProjectAddInput,
    options: { local: boolean },
  ) => Promise<void>;
}

function errorText(error: unknown): string {
  if (
    error &&
    typeof error === "object" &&
    "message" in error &&
    typeof error.message === "string" &&
    error.message
  ) {
    return error.message;
  }
  return "The project could not be added. Try again.";
}

const NAME_PATTERN = /^[a-z0-9][a-z0-9._-]{0,63}$/;

/**
 * Add a project to the connected server, like DeepSeek Harness's "Choose
 * workspace": a name and where the code comes from. No secrets are
 * asked for; private repositories use the server's own Git credentials.
 */
export function AddProjectDialog({
  serverLabel,
  existing,
  localNode,
  onClose,
  onCreate,
}: AddProjectDialogProps) {
  const [kind, setKind] = useState<"git" | "folder">("git");
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [path, setPath] = useState("");
  const ids = { name: useId(), url: useId(), path: useId(), local: useId() };
  const [local, setLocal] = useState(localNode !== null);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);

  // Suggest a name from the repository URL until the user types one.
  const [nameTouched, setNameTouched] = useState(false);
  const suggested =
    url
      .trim()
      .replace(/\.git$/, "")
      .split(/[/:]/)
      .pop()
      ?.toLowerCase()
      .replace(/[^a-z0-9._-]/g, "-") ?? "";
  const effectiveName = nameTouched ? name.trim() : name.trim() || suggested;

  const nameError = !effectiveName
    ? null
    : !NAME_PATTERN.test(effectiveName)
      ? "Use lowercase letters, numbers, dots, dashes or underscores."
      : existing.includes(effectiveName)
        ? "A project with this name already exists on this server."
        : null;
  const sourceReady =
    kind === "git"
      ? /^(https:\/\/|git@|ssh:\/\/)\S+$/.test(url.trim())
      : path.trim().startsWith("/") || path.trim().startsWith("~");
  const ready = Boolean(effectiveName) && !nameError && sourceReady;

  async function submit(event?: FormEvent) {
    event?.preventDefault();
    if (!ready || busy) return;
    setBusy(true);
    setFailure(null);
    try {
      await onCreate(
        {
          name: effectiveName,
          source:
            kind === "git"
              ? { kind, url: url.trim() }
              : { kind, path: path.trim() },
        },
        { local: localNode !== null && local },
      );
    } catch (error) {
      setFailure(errorText(error));
      setBusy(false);
    }
  }

  return (
    <Modal
      open
      title="Add a project"
      onClose={onClose}
      footer={
        <>
          <button
            type="button"
            className="nx-btn nx-btn--quiet"
            onClick={onClose}
            disabled={busy}
          >
            Cancel
          </button>
          <button
            type="button"
            className="nx-btn"
            disabled={!ready || busy}
            onClick={() => void submit()}
          >
            {busy ? "Creating project…" : "Create project"}
          </button>
        </>
      }
    >
      <form
        className="nx-form"
        onSubmit={(event) => void submit(event)}
        aria-label="New project"
      >
        <p className="nx-form__lede">
          The project lives on <strong>{serverLabel}</strong>. Its orchestrator
          and workers run there.
        </p>

        <div className="nx-form__row">
          <span className="nx-form__label" id={`${ids.url}-source`}>
            Where is the code?
          </span>
          <div
            className="nx-segment nx-segment--block"
            role="group"
            aria-labelledby={`${ids.url}-source`}
          >
            <button
              type="button"
              aria-pressed={kind === "git"}
              onClick={() => setKind("git")}
            >
              Git repository
            </button>
            <button
              type="button"
              aria-pressed={kind === "folder"}
              onClick={() => setKind("folder")}
            >
              Folder on the server
            </button>
          </div>
        </div>

        {kind === "git" ? (
          <>
            <label className="nx-field" htmlFor={ids.url}>
              Repository URL
              <input
                id={ids.url}
                value={url}
                onChange={(event) => setUrl(event.target.value)}
                placeholder="https://github.com/you/project.git"
                spellCheck={false}
                autoComplete="off"
                aria-describedby={`${ids.url}-note`}
              />
            </label>
            <p className="nx-form__hint" id={`${ids.url}-note`}>
              StackSpirit clones the repository's default branch.
            </p>
          </>
        ) : (
          <label className="nx-field" htmlFor={ids.path}>
            Folder path on {serverLabel}
            <input
              id={ids.path}
              value={path}
              onChange={(event) => setPath(event.target.value)}
              placeholder="/home/you/code/project"
              spellCheck={false}
              autoComplete="off"
            />
          </label>
        )}

        <label className="nx-field" htmlFor={ids.name}>
          Project name
          <input
            id={ids.name}
            value={nameTouched ? name : name || suggested}
            onChange={(event) => {
              setNameTouched(true);
              setName(event.target.value);
            }}
            placeholder="my-project"
            spellCheck={false}
            autoComplete="off"
            aria-invalid={nameError ? true : undefined}
          />
          {nameError ? (
            <span className="nx-form__error">{nameError}</span>
          ) : null}
        </label>

        <div className="nx-form__check">
          <input
            id={ids.local}
            type="checkbox"
            checked={localNode !== null && local}
            disabled={localNode === null || busy}
            onChange={(event) => setLocal(event.target.checked)}
            aria-describedby={`${ids.local}-note`}
          />
          <label htmlFor={ids.local}>
            Create the chat on this Mac{localNode ? ` (${localNode})` : ""}
          </label>
          <span className="nx-form__hint" id={`${ids.local}-note`}>
            {localNode
              ? "The project chat starts on your connected Mac instead of staying on the server."
              : "No local agent is connected, so the chat will stay on the server."}
          </span>
        </div>

        {failure ? (
          <p className="nx-form__error nx-form__error--block" role="alert">
            {failure}
          </p>
        ) : null}

        <div className="nx-secure__note">
          <span aria-hidden="true">🔒</span>
          <span>
            Private repositories use the server's own Git login (for example a
            deploy key). StackSpirit never copies your GitHub credentials to the
            server.
          </span>
        </div>
      </form>
    </Modal>
  );
}
