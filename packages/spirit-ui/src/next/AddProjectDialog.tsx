import { useId, useState, type FormEvent } from "react";

import { Modal } from "../components";
import type { ProjectRequest } from "./conversations";

export interface AddProjectDialogProps {
  serverLabel: string;
  existing: string[];
  onClose: () => void;
  onCreate: (request: ProjectRequest) => void;
}

const NAME_PATTERN = /^[a-z0-9][a-z0-9._-]{0,63}$/;

/**
 * Add a project to the connected server, like DeepSeek Harness's "Choose
 * workspace": a name, where the code comes from, and the branch. No secrets are
 * asked for; private repositories use the server's own Git credentials.
 */
export function AddProjectDialog({ serverLabel, existing, onClose, onCreate }: AddProjectDialogProps) {
  const [kind, setKind] = useState<"git" | "folder">("git");
  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [branch, setBranch] = useState("main");
  const [path, setPath] = useState("");
  const ids = { name: useId(), url: useId(), branch: useId(), path: useId() };

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
    kind === "git" ? /^(https:\/\/|git@|ssh:\/\/)\S+$/.test(url.trim()) : path.trim().startsWith("/") || path.trim().startsWith("~");
  const ready = Boolean(effectiveName) && !nameError && sourceReady;

  function submit(event?: FormEvent) {
    event?.preventDefault();
    if (!ready) return;
    onCreate({
      name: effectiveName,
      source: kind === "git" ? { kind, url: url.trim(), branch: branch.trim() || "main" } : { kind, path: path.trim() },
    });
  }

  return (
    <Modal
      open
      title="Add a project"
      onClose={onClose}
      footer={
        <>
          <button type="button" className="nx-btn nx-btn--quiet" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="nx-btn" disabled={!ready} onClick={() => submit()}>
            Create project
          </button>
        </>
      }
    >
      <form className="nx-form" onSubmit={submit} aria-label="New project">
        <p className="nx-form__lede">
          The project lives on <strong>{serverLabel}</strong>. Its orchestrator and workers run there.
        </p>

        <div className="nx-form__row">
          <span className="nx-form__label" id={`${ids.url}-source`}>
            Where is the code?
          </span>
          <div className="nx-segment nx-segment--block" role="group" aria-labelledby={`${ids.url}-source`}>
            <button type="button" aria-pressed={kind === "git"} onClick={() => setKind("git")}>
              Git repository
            </button>
            <button type="button" aria-pressed={kind === "folder"} onClick={() => setKind("folder")}>
              Folder on the server
            </button>
          </div>
        </div>

        {kind === "git" ? (
          <div className="nx-form__grid">
            <label className="nx-field" htmlFor={ids.url}>
              Repository URL
              <input
                id={ids.url}
                value={url}
                onChange={(event) => setUrl(event.target.value)}
                placeholder="https://github.com/you/project.git"
                spellCheck={false}
                autoComplete="off"
              />
            </label>
            <label className="nx-field" htmlFor={ids.branch}>
              Branch
              <input id={ids.branch} value={branch} onChange={(event) => setBranch(event.target.value)} spellCheck={false} />
            </label>
          </div>
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
          {nameError ? <span className="nx-form__error">{nameError}</span> : null}
        </label>

        <div className="nx-secure__note">
          <span aria-hidden="true">🔒</span>
          <span>
            Private repositories use the server's own Git login (for example a deploy key). StackSpirit
            never copies your GitHub credentials to the server.
          </span>
        </div>
      </form>
    </Modal>
  );
}
