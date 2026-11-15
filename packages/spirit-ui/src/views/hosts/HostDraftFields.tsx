import type { HostDraft } from "../../bridge/wire";

export interface DraftErrors {
  label?: string;
  host?: string;
  user?: string;
  port?: string;
  vowp_home?: string;
  default_project?: string;
}

export function validateDraft(draft: HostDraft): DraftErrors {
  const errors: DraftErrors = {};
  const label = draft.label.trim();
  if (label.length === 0 || label.length > 64) {
    errors.label = "Label must be 1–64 characters.";
  }
  const host = draft.ssh.host.trim();
  if (!host) {
    errors.host = "Enter a DNS name or IP address.";
  } else if (/\s|@|\/\//.test(host)) {
    errors.host = "Use a DNS name or IP address without a scheme, '@', or spaces.";
  }
  if (!draft.ssh.user.trim()) {
    errors.user = "Enter the SSH user.";
  } else if (!/^[A-Za-z0-9._-]+$/.test(draft.ssh.user) || draft.ssh.user.startsWith("-")) {
    errors.user = "Use letters, digits, '.', '_' and '-' only.";
  }
  if (
    !Number.isInteger(draft.ssh.port) ||
    draft.ssh.port < 1 ||
    draft.ssh.port > 65535
  ) {
    errors.port = "Port must be between 1 and 65535.";
  }
  if (draft.vowp_home && !draft.vowp_home.startsWith("/")) {
    errors.vowp_home = "VOWP home must be an absolute path, or empty.";
  }
  if (draft.default_project && !/^[a-z0-9][a-z0-9._-]*$/i.test(draft.default_project)) {
    errors.default_project = "Use a project slug, or leave it empty.";
  }
  return errors;
}

export interface HostDraftFieldsProps {
  draft: HostDraft;
  errors: DraftErrors;
  onChange: (draft: HostDraft) => void;
  disabled?: boolean;
}

export function HostDraftFields({
  draft,
  errors,
  onChange,
  disabled = false,
}: HostDraftFieldsProps) {
  function patch(patchValue: Partial<HostDraft>) {
    onChange({ ...draft, ...patchValue });
  }
  function patchSsh(patchValue: Partial<HostDraft["ssh"]>) {
    onChange({ ...draft, ssh: { ...draft.ssh, ...patchValue } });
  }

  return (
    <div className="form-grid">
      <label className="field">
        <span className="field__label">Label *</span>
        <input
          className="input"
          value={draft.label}
          disabled={disabled}
          aria-invalid={Boolean(errors.label)}
          aria-describedby={errors.label ? "label-error" : undefined}
          placeholder="staging-vps"
          onChange={(event) => patch({ label: event.target.value })}
        />
        {errors.label ? (
          <span id="label-error" className="field__error" role="alert">
            {errors.label}
          </span>
        ) : null}
      </label>

      <label className="field">
        <span className="field__label">Host *</span>
        <input
          className="input mono"
          value={draft.ssh.host}
          disabled={disabled}
          aria-invalid={Boolean(errors.host)}
          aria-describedby={errors.host ? "host-error" : undefined}
          placeholder="203.0.113.10"
          onChange={(event) => patchSsh({ host: event.target.value })}
        />
        {errors.host ? (
          <span id="host-error" className="field__error" role="alert">
            {errors.host}
          </span>
        ) : null}
      </label>

      <div className="form-row">
        <label className="field">
          <span className="field__label">User *</span>
          <input
            className="input mono"
            value={draft.ssh.user}
            disabled={disabled}
            aria-invalid={Boolean(errors.user)}
            aria-describedby={errors.user ? "user-error" : undefined}
            placeholder="owner"
            onChange={(event) => patchSsh({ user: event.target.value })}
          />
          {errors.user ? (
            <span id="user-error" className="field__error" role="alert">
              {errors.user}
            </span>
          ) : null}
        </label>

        <label className="field field--narrow">
          <span className="field__label">Port *</span>
          <input
            className="input mono"
            type="number"
            min={1}
            max={65535}
            value={Number.isFinite(draft.ssh.port) ? draft.ssh.port : ""}
            disabled={disabled}
            aria-invalid={Boolean(errors.port)}
            aria-describedby={errors.port ? "port-error" : undefined}
            onChange={(event) => patchSsh({ port: Number(event.target.value) })}
          />
          {errors.port ? (
            <span id="port-error" className="field__error" role="alert">
              {errors.port}
            </span>
          ) : null}
        </label>
      </div>

      <label className="field">
        <span className="field__label">Identity file</span>
        <input
          className="input mono"
          value={draft.identity_file ?? ""}
          disabled={disabled}
          placeholder="~/.ssh/id_ed25519 (path only)"
          onChange={(event) =>
            patch({ identity_file: event.target.value === "" ? null : event.target.value })
          }
        />
        <span className="field__hint">
          Leave empty to use your SSH agent. StackSpirit never stores a passphrase or a
          private key.
        </span>
      </label>

      <label className="field">
        <span className="field__label">VOWP home</span>
        <input
          className="input mono"
          value={draft.vowp_home ?? ""}
          disabled={disabled}
          aria-invalid={Boolean(errors.vowp_home)}
          placeholder="/home/owner/.vowp (remote default when empty)"
          onChange={(event) =>
            patch({ vowp_home: event.target.value === "" ? null : event.target.value })
          }
        />
        {errors.vowp_home ? (
          <span className="field__error" role="alert">
            {errors.vowp_home}
          </span>
        ) : null}
        <span className="field__hint">Passed to VOWP as `--home`.</span>
      </label>

      <label className="field">
        <span className="field__label">Default project</span>
        <input
          className="input mono"
          value={draft.default_project ?? ""}
          disabled={disabled}
          aria-invalid={Boolean(errors.default_project)}
          placeholder="all projects when empty"
          onChange={(event) =>
            patch({
              default_project: event.target.value === "" ? null : event.target.value,
            })
          }
        />
        {errors.default_project ? (
          <span className="field__error" role="alert">
            {errors.default_project}
          </span>
        ) : null}
        <span className="field__hint">Only project-scoped actions pass `--project`.</span>
      </label>
    </div>
  );
}

export function emptyDraft(): HostDraft {
  return {
    label: "",
    ssh: { user: "", host: "", port: 22 },
    identity_file: null,
    vowp_home: null,
    default_project: null,
  };
}
