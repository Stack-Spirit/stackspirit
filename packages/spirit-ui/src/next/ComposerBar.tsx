import {
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ClipboardEvent,
  type DragEvent,
  type KeyboardEvent,
  type ReactNode,
} from "react";

import type {
  ChatAttachment,
  ChatModelView,
  ChatUsageView,
  SessionId,
} from "../bridge/wire";
import { Modal } from "../components/Modal";
import { ContextMeter, UsageStats } from "./ContextMeter";
import { SettingsPage } from "./SettingsPage";

/**
 * How a model is named in the composer. Included free models are labelled with the
 * bundled engine's OpenCode free tier; a BYOK or orchestrator-owned choice never
 * borrows that label.
 */
export function modelDisplayLabel(model: ChatModelView | null): string {
  if (!model) return "No model";
  if (model.tier === "included") return `OpenCode free · ${model.label}`;
  if (model.tier === "byok") return `BYOK · ${model.label}`;
  if (model.tier === "orchestrator") return `Orchestrator · ${model.label}`;
  return model.label;
}

/**
 * The reasoning effort a model starts on, like OpenCode: `medium`, else `high`, else the
 * first variant the model declares. A model without variants has no effort (`null`).
 */
export function defaultVariant(variants: string[]): string | null {
  if (variants.length === 0) return null;
  if (variants.includes("medium")) return "medium";
  if (variants.includes("high")) return "high";
  return variants[0]!;
}

/**
 * Provider order the connect dialog uses (mirrors `POPULAR_PROVIDER_IDS` in
 * `crates/spirit-core/src/engine/providers.rs`). Connected sections follow it, then
 * fall back to alphabetical by display label.
 */
export const POPULAR_PROVIDER_IDS = [
  "opencode",
  "opencode-go",
  "anthropic",
  "github-copilot",
  "openai",
  "google",
  "openrouter",
  "vercel",
] as const;

const PROVIDER_LABELS: Record<string, string> = {
  opencode: "OpenCode",
  "opencode-go": "OpenCode Go",
  anthropic: "Anthropic",
  openai: "OpenAI",
  google: "Google",
  openrouter: "OpenRouter",
  deepseek: "DeepSeek",
  "github-copilot": "GitHub Copilot",
  vercel: "Vercel",
};

/** A readable provider name for a provider id the engine reports. */
export function providerDisplayLabel(provider: string): string {
  const known = PROVIDER_LABELS[provider];
  if (known) return known;
  return provider ? provider.charAt(0).toUpperCase() + provider.slice(1) : "Other";
}

export interface ChatModelGroup {
  /** `included` for the bundled free models, otherwise the provider id. */
  id: string;
  title: string;
  tier: "included" | "byok" | "orchestrator";
  models: ChatModelView[];
}

function popularRank(provider: string): number {
  const index = (POPULAR_PROVIDER_IDS as readonly string[]).indexOf(provider);
  return index === -1 ? POPULAR_PROVIDER_IDS.length : index;
}

/**
 * Groups the verified chat model list the way the picker shows it: the bundled free
 * models first under "OpenCode Free", then one named section per connected provider,
 * popular providers first and the rest alphabetical. Empty groups are never produced.
 */
export function groupChatModels(models: ChatModelView[]): ChatModelGroup[] {
  const included = models.filter((model) => model.tier === "included");
  const byProvider = new Map<string, ChatModelView[]>();
  for (const model of models) {
    if (model.tier === "included") continue;
    const list = byProvider.get(model.provider) ?? [];
    list.push(model);
    byProvider.set(model.provider, list);
  }
  const sections: ChatModelGroup[] = [...byProvider.entries()].map(([provider, list]) => ({
    id: provider,
    title: providerDisplayLabel(provider),
    tier: list.every((model) => model.tier === "orchestrator") ? "orchestrator" : "byok",
    models: list,
  }));
  sections.sort(
    (a, b) =>
      popularRank(a.id) - popularRank(b.id) ||
      a.title.localeCompare(b.title, undefined, { sensitivity: "base" }),
  );
  return [
    ...(included.length > 0
      ? [{ id: "included", title: "OpenCode Free", tier: "included" as const, models: included }]
      : []),
    ...sections,
  ];
}

/** Case-insensitive, multi-word substring match over label, id and provider name/id. */
export function filterChatModels(models: ChatModelView[], query: string): ChatModelView[] {
  const tokens = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (tokens.length === 0) return models;
  return models.filter((model) => {
    const haystack = [
      model.label,
      model.id,
      model.provider,
      providerDisplayLabel(model.provider),
      model.tier === "included" ? "opencode free" : "",
    ]
      .join(" ")
      .toLowerCase();
    return tokens.every((token) => haystack.includes(token));
  });
}

/** The secondary id line: the id without its provider prefix, when it adds information. */
function modelSubId(model: ChatModelView): string | null {
  if (model.tier === "included") return null;
  const prefix = `${model.provider}/`;
  const rest = model.id.startsWith(prefix) ? model.id.slice(prefix.length) : model.id;
  return rest && rest !== model.label ? rest : null;
}

function tierBadge(tier: ChatModelGroup["tier"]): string {
  if (tier === "included") return "Free";
  if (tier === "orchestrator") return "Orchestrator";
  return "Connected";
}

/** Largest single attachment, in bytes. */
export const MAX_ATTACHMENT_BYTES = 5 * 1024 * 1024;
/** Most attachments on one turn. */
export const MAX_ATTACHMENTS = 8;
const ACCEPT =
  "image/*,application/pdf,text/*,.md,.json,.yaml,.yml,.toml,.rs,.ts,.tsx,.js,.jsx,.py,.go,.java,.c,.cpp,.h,.sh,.css,.html,.sql,.log";

/** Reads one file as a `data:` URI attachment. */
function readAttachment(file: File): Promise<ChatAttachment> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error(`Could not read ${file.name}.`));
    reader.onload = () => {
      const mime = file.type || "text/plain";
      let url = String(reader.result);
      // Some platforms leave the MIME empty in the data URI.
      if (url.startsWith("data:;")) url = `data:${mime};${url.slice(6)}`;
      resolve({ filename: file.name || "pasted-image", mime, url });
    };
    reader.readAsDataURL(file);
  });
}

function capitalize(value: string): string {
  return value.charAt(0).toUpperCase() + value.slice(1);
}

interface PendingAttachment extends ChatAttachment {
  key: string;
}

export interface ComposerBarProps {
  session: SessionId;
  placeholder: string;
  /** The model the session reports, or null when it reports none. */
  model: ChatModelView | null;
  /**
   * The verified usable list. `undefined` means the transport cannot list models
   * (an orchestrator owns its config); an empty array is an honest "none".
   */
  models?: ChatModelView[];
  /** True only when the engine genuinely reports zero usable models. */
  modelMissing: boolean;
  disabled: boolean;
  streaming: boolean;
  /** A turn is being accepted; the primary button shows a spinner. */
  sending?: boolean;
  stopped: boolean;
  /**
   * Keep the inline model picker usable while the composer is otherwise locked
   * (a retrying turn or a recoverable error). The textarea and Send stay disabled.
   */
  allowModelSwitch?: boolean;
  /** Bump this counter to open and focus the model picker from a recovery notice. */
  openModelPickerRequest?: number;
  /** True while the card sits in the centered hero; false when docked. */
  hero?: boolean;
  usage?: ChatUsageView | null;
  /** May return the acceptance promise so the primary button can show a spinner. */
  onSend: (
    text: string,
    options?: { variant?: string | null; attachments?: ChatAttachment[] },
  ) => void | Promise<void>;
  onStop: () => void;
  onStart: () => void;
  onSelectModel?: (modelId: string) => void;
  /** Reveals the app-owned secure server card; never opens it on its own. */
  onConnectServer?: () => void;
  showConnectServer?: boolean;
  onOpenEngineSettings?: () => void;
  /** An editable draft to place in the field (e.g. an inbox discussion). Applied once per nonce. */
  draft?: { text: string; nonce: number } | null;
  onDraftApplied?: () => void;
}

/**
 * The elevated Meadow composer card.
 *
 * One instance stays mounted in the empty hero and in the docked layout, so moving
 * from the centre to the bottom of the chat never remounts the textarea. If a turn
 * disables it, the field regains focus when the session returns to ready. The left
 * control row owns the inline model picker and the compact connect action; the right
 * owns the single 32 px circular Send / Stop button.
 */
export function ComposerBar({
  session,
  placeholder,
  model,
  models,
  modelMissing,
  disabled,
  streaming,
  sending = false,
  stopped,
  allowModelSwitch = false,
  openModelPickerRequest = 0,
  hero = false,
  usage,
  onSend,
  onStop,
  onStart,
  onSelectModel,
  onConnectServer,
  showConnectServer = false,
  onOpenEngineSettings,
  draft = null,
  onDraftApplied,
}: ComposerBarProps) {
  const [text, setText] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [providerSettingsOpen, setProviderSettingsOpen] = useState(false);
  const [attachments, setAttachments] = useState<PendingAttachment[]>([]);
  const [attachError, setAttachError] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [variantByModel, setVariantByModel] = useState<Record<string, string>>({});
  const fileInput = useRef<HTMLInputElement>(null);
  const textarea = useRef<HTMLTextAreaElement>(null);
  /** True while the Message field owns (or is about to regain) focus. */
  const restoreFocus = useRef(false);
  /** True after the Stop button click, so the field regains focus when the turn ends. */
  const stopRequested = useRef(false);
  const id = useId();

  useEffect(() => {
    const element = textarea.current;
    if (!element) return;
    element.style.height = "auto";
    const next = Math.min(Math.max(element.scrollHeight, 24), 200);
    element.style.height = `${next}px`;
    // `hero` is a dependency so the two-line hero floor collapses back to the
    // one-line docked height when the first turn moves the card down.
  }, [text, hero]);

  // A disabled control cannot keep DOM focus. When a turn starts the field is
  // disabled, so the browser moves focus to BODY; return it once the session is
  // ready again, unless the owner deliberately focused another control meanwhile.
  useEffect(() => {
    const element = textarea.current;
    if (!element || disabled || !restoreFocus.current) return;
    restoreFocus.current = false;
    if (document.activeElement === document.body) element.focus();
  }, [disabled]);

  useEffect(() => {
    const element = textarea.current;
    if (!element || disabled || !stopRequested.current) return;
    stopRequested.current = false;
    element.focus();
  }, [disabled]);

  const appliedDraft = useRef(0);
  useEffect(() => {
    if (!draft || draft.nonce === appliedDraft.current) return;
    appliedDraft.current = draft.nonce;
    setText(draft.text);
    onDraftApplied?.();
    const element = textarea.current;
    if (element && !element.disabled) {
      element.focus();
      element.setSelectionRange(draft.text.length, draft.text.length);
    }
  }, [draft, onDraftApplied]);

  // Reasoning effort is strictly the selected model's own declared variants.
  const modelVariants = useMemo(() => {
    const found = models?.find((candidate) => candidate.id === model?.id) ?? model;
    return found?.variants ?? [];
  }, [models, model]);
  const chosen = model ? variantByModel[model.id] : undefined;
  const activeVariant =
    chosen && modelVariants.includes(chosen) ? chosen : defaultVariant(modelVariants);

  async function addFiles(files: File[]) {
    if (files.length === 0) return;
    setAttachError(null);
    const next: PendingAttachment[] = [];
    for (const file of files) {
      if (attachments.length + next.length >= MAX_ATTACHMENTS) {
        setAttachError(`You can attach up to ${MAX_ATTACHMENTS} files per message.`);
        break;
      }
      if (file.size > MAX_ATTACHMENT_BYTES) {
        setAttachError(
          `${file.name || "File"} is too large. The limit is ${MAX_ATTACHMENT_BYTES / (1024 * 1024)} MB per file.`,
        );
        continue;
      }
      try {
        const item = await readAttachment(file);
        next.push({ ...item, key: `${Date.now()}-${Math.random().toString(36).slice(2)}` });
      } catch (caught) {
        setAttachError(caught instanceof Error ? caught.message : "Could not read the file.");
      }
    }
    if (next.length > 0) setAttachments((current) => [...current, ...next]);
  }

  function onPaste(event: ClipboardEvent<HTMLTextAreaElement>) {
    const files = Array.from(event.clipboardData?.files ?? []).filter((file) =>
      file.type.startsWith("image/"),
    );
    if (files.length === 0) return;
    event.preventDefault();
    void addFiles(files);
  }

  function onDrop(event: DragEvent<HTMLFormElement>) {
    event.preventDefault();
    setDragging(false);
    if (disabled) return;
    void addFiles(Array.from(event.dataTransfer?.files ?? []));
  }

  async function submit() {
    if (disabled || streaming || sending || submitting) return;
    const trimmed = text.trim();
    if (!trimmed) return;
    setText("");
    const sentAttachments = attachments.map(({ filename, mime, url }) => ({
      filename,
      mime,
      url,
    }));
    setAttachments([]);
    setAttachError(null);
    const pending = onSend(trimmed, {
      variant: activeVariant,
      ...(sentAttachments.length > 0 ? { attachments: sentAttachments } : {}),
    });
    if (pending) {
      setSubmitting(true);
      try {
        await pending;
      } finally {
        setSubmitting(false);
      }
    }
  }

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      void submit();
    }
  }

  const busy = sending || submitting;
  const canSend = !disabled && !streaming && !busy && text.trim() !== "";
  const showPicker = !modelMissing && models !== undefined && models.length > 0;
  const showStaticModel = !modelMissing && !showPicker;
  const connect = session === "setup" && onConnectServer !== undefined && showConnectServer;

  return (
    <div className="nx-composer-shell">
      <form
        className={`nx-composer${disabled ? " nx-composer--disabled" : ""}${
          dragging ? " nx-composer--drag" : ""
        }`}
        aria-busy={sending}
        data-testid="composer-form"
        onDragOver={(event) => {
          if (disabled) return;
          event.preventDefault();
          setDragging(true);
        }}
        onDragLeave={() => setDragging(false)}
        onDrop={onDrop}
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
      >
      {attachments.length > 0 ? (
        <ul className="nx-attachments" aria-label="Attachments" data-testid="composer-attachments">
          {attachments.map((item) => (
            <li key={item.key} className="nx-attachment">
              {item.mime.startsWith("image/") ? (
                <img className="nx-attachment__thumb" src={item.url} alt="" />
              ) : (
                <span className="nx-attachment__icon" aria-hidden="true">
                  ▤
                </span>
              )}
              <span className="nx-attachment__name" title={item.filename}>
                {item.filename}
              </span>
              <button
                type="button"
                className="nx-attachment__remove"
                aria-label={`Remove ${item.filename}`}
                onClick={() =>
                  setAttachments((current) => current.filter((entry) => entry.key !== item.key))
                }
              >
                ×
              </button>
            </li>
          ))}
        </ul>
      ) : null}
      {attachError ? (
        <p className="nx-attachments__error" role="alert" data-testid="composer-attach-error">
          {attachError}
        </p>
      ) : null}
      <label htmlFor={id} className="nx-sr">
        Message
      </label>
      <textarea
        ref={textarea}
        id={id}
        rows={1}
        value={text}
        placeholder={placeholder}
        disabled={disabled}
        onChange={(event) => setText(event.target.value)}
        onFocus={() => {
          restoreFocus.current = true;
        }}
        onBlur={(event) => {
          // The browser blurs the field as it becomes disabled; remember the
          // intent so it can be focused again when the turn finishes. A real
          // focus move while enabled clears it.
          if (!event.currentTarget.disabled) restoreFocus.current = false;
        }}
        onKeyDown={onKeyDown}
        onPaste={onPaste}
      />
      <div className="nx-composer__controls">
        <div className="nx-composer__left">
          <input
            ref={fileInput}
            type="file"
            multiple
            hidden
            accept={ACCEPT}
            data-testid="composer-file-input"
            aria-label="Attach files"
            tabIndex={-1}
            onChange={(event) => {
              const files = Array.from(event.target.files ?? []);
              event.target.value = "";
              void addFiles(files);
            }}
          />
          <button
            type="button"
            className="nx-composer__attach"
            data-testid="composer-attach"
            aria-label="Attach images or files"
            title="Attach images or files"
            disabled={disabled}
            onClick={() => fileInput.current?.click()}
          >
            <span aria-hidden="true">+</span>
          </button>
          {modelMissing ? (
            <button
              type="button"
              className="nx-composer__setup"
              data-testid="composer-setup-model"
              onClick={onOpenEngineSettings}
              disabled={!onOpenEngineSettings}
            >
              Set up a model
            </button>
          ) : null}
          {showPicker ? (
            <ModelPicker
              model={model}
              models={models}
              variants={modelVariants}
              variant={activeVariant}
              onSelectVariant={(value) =>
                model && setVariantByModel((current) => ({ ...current, [model.id]: value }))
              }
              disabled={disabled && !allowModelSwitch}
              openRequest={openModelPickerRequest}
              onSelect={onSelectModel}
              onConnectProvider={() => setProviderSettingsOpen(true)}
              onManageModels={() => setProviderSettingsOpen(true)}
            />
          ) : null}
          {showStaticModel ? (
            <span
              className="nx-composer__model-static"
              data-testid="composer-model-static"
              title={
                models === undefined
                  ? "The orchestrator owns its own model configuration"
                  : "The model the chat session reports"
              }
            >
              {model ? modelDisplayLabel(model) : "Orchestrator default"}
              {activeVariant ? ` · ${capitalize(activeVariant)}` : ""}
            </span>
          ) : null}
          {connect ? (
            <button
              type="button"
              className="nx-composer__connect"
              data-testid="composer-connect-server"
              onClick={onConnectServer}
            >
              <span aria-hidden="true">＋</span>
              Connect server
            </button>
          ) : null}
          {stopped ? (
            <button type="button" className="nx-composer__start" onClick={onStart}>
              Start
            </button>
          ) : null}
        </div>
        <div className="nx-composer__right">
          {streaming ? (
            <button
              type="button"
              className="nx-send nx-send--stop"
              aria-label="Stop"
              onClick={() => {
                stopRequested.current = true;
                onStop();
              }}
            >
              <span className="nx-send__stop" aria-hidden="true" />
            </button>
          ) : (
            <button
              type="submit"
              className="nx-send"
              aria-label="Send"
              disabled={!canSend}
            >
              {busy ? (
                <span className="nx-send__spinner" aria-hidden="true" />
              ) : (
                <svg
                  width="16"
                  height="16"
                  viewBox="0 0 16 16"
                  aria-hidden="true"
                  focusable="false"
                >
                  <path
                    d="M8 13V3M4 7l4-4 4 4"
                    fill="none"
                    stroke="currentColor"
                    strokeWidth="1.8"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  />
                </svg>
              )}
            </button>
          )}
        </div>
        </div>
      </form>
      <ComposerDock usage={usage} />
      {providerSettingsOpen ? (
        <Modal
          open
          title="Engine & Models"
          onClose={() => setProviderSettingsOpen(false)}
        >
          <SettingsPage />
        </Modal>
      ) : null}
    </div>
  );
}

/** The dock row under the card; it always reserves 22 px so metrics never shift layout. */
export function ComposerDock({ usage }: { usage?: ChatUsageView | null }) {
  return (
    <div className="nx-composer-dock" data-testid="composer-dock">
      <ContextMeter usage={usage} />
      <UsageStats usage={usage} />
    </div>
  );
}

function ModelPicker({
  model,
  models,
  variants = [],
  variant = null,
  onSelectVariant,
  disabled,
  openRequest = 0,
  onSelect,
  onConnectProvider,
  onManageModels,
}: {
  model: ChatModelView | null;
  models: ChatModelView[];
  variants?: string[];
  variant?: string | null;
  onSelectVariant?: (variant: string) => void;
  disabled: boolean;
  openRequest?: number;
  onSelect?: (modelId: string) => void;
  onConnectProvider?: () => void;
  onManageModels?: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState<string | null>(null);
  const [maxHeight, setMaxHeight] = useState<number | null>(null);
  const [view, setView] = useState<"root" | "models" | "effort">("root");
  const id = useId();
  const modelRow = useRef<HTMLButtonElement>(null);
  const effortRow = useRef<HTMLButtonElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const search = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLDivElement>(null);

  function openPicker(initial: "root" | "models" = "root") {
    setQuery("");
    setActive(null);
    setView(initial);
    setOpen(true);
  }

  useEffect(() => {
    if (openRequest <= 0) return;
    // A recovery notice asks for the picker after its click handler; defer one tick so
    // the popover can measure the trigger before it opens.
    const timer = setTimeout(() => openPicker("models"), 0);
    return () => clearTimeout(timer);
  }, [openRequest]);

  // Cap the popover to the room between the trigger and the page header so the
  // sticky search field never slides under the server banner; only the list scrolls.
  useLayoutEffect(() => {
    if (!open) return;
    const measure = () => {
      const top = trigger.current?.getBoundingClientRect().top;
      if (top === undefined) return;
      let inset = 0;
      // The popover is clipped by .nx-chat, which sits below the pinned saved-server banner.
      const clip = trigger.current?.closest(".nx-chat");
      if (clip) inset = clip.getBoundingClientRect().top;
      for (const head of document.querySelectorAll(".nx-chat-head")) {
        inset = Math.max(inset, head.getBoundingClientRect().bottom);
      }
      setMaxHeight(Math.max(140, Math.floor(top - inset - 6 - 8)));
    };
    measure();
    window.addEventListener("resize", measure);
    return () => window.removeEventListener("resize", measure);
  }, [open]);

  const focusRowAfterBack = useRef<"model" | "effort" | null>(null);
  useEffect(() => {
    if (!open) return;
    if (view === "models") search.current?.focus();
    else if (view === "root") {
      const row = focusRowAfterBack.current === "effort" ? effortRow : modelRow;
      focusRowAfterBack.current = null;
      row.current?.focus();
    }
  }, [open, view]);

  useEffect(() => {
    if (!open) return;
    const onPointer = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") {
        if (view !== "root") {
          focusRowAfterBack.current = view === "effort" ? "effort" : "model";
          setView("root");
          return;
        }
        setOpen(false);
        trigger.current?.focus();
      }
    };
    document.addEventListener("pointerdown", onPointer);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onPointer);
      document.removeEventListener("keydown", onKey);
    };
  }, [open, view]);

  const groups = useMemo(
    () =>
      open && view === "models" ? groupChatModels(filterChatModels(models, query)) : [],
    [open, view, models, query],
  );
  const visible = useMemo(() => groups.flatMap((group) => group.models), [groups]);
  const highlighted =
    visible.find((candidate) => candidate.id === active)?.id ??
    visible.find((candidate) => candidate.id === model?.id && query.trim() === "")?.id ??
    visible[0]?.id ??
    null;
  const optionId = (modelId: string) => `${id}-opt-${modelId}`;

  useEffect(() => {
    if (!open || !highlighted) return;
    document
      .getElementById(`${id}-opt-${highlighted}`)
      ?.scrollIntoView?.({ block: "nearest" });
  }, [open, highlighted, id]);

  function choose(modelId: string) {
    setOpen(false);
    trigger.current?.focus();
    onSelect?.(modelId);
  }

  function onSearchKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (visible.length === 0) return;
      const index = visible.findIndex((candidate) => candidate.id === highlighted);
      const step = event.key === "ArrowDown" ? 1 : -1;
      const next = (index + step + visible.length) % visible.length;
      setActive(visible[next]!.id);
    } else if (event.key === "Enter") {
      event.preventDefault();
      if (highlighted) choose(highlighted);
    } else if (event.key === "Escape") {
      // A search field clears itself on Escape; the picker closes instead.
      event.preventDefault();
    }
  }

  const current = model ? modelDisplayLabel(model) : "Choose a model";
  const currentWithEffort = variant ? `${current} · ${capitalize(variant)}` : current;
  const trimmed = query.trim();

  return (
    <div className="nx-menu nx-model-picker" ref={root}>
      <button
        ref={trigger}
        type="button"
        className="nx-model-picker__trigger"
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        aria-label={`Model: ${currentWithEffort}`}
        data-testid="composer-model"
        title={currentWithEffort}
        disabled={disabled || !onSelect}
        onClick={() => (open ? setOpen(false) : openPicker())}
      >
        <span className="nx-model-picker__label">{currentWithEffort}</span>
        <Chevron />
      </button>
      {open ? (
        <div
          id={id}
          className="nx-menu__pop nx-menu__pop--above nx-menu__pop--start nx-model-menu"
          role="dialog"
          aria-label="Choose a model"
          data-testid="model-picker-popover"
          style={maxHeight === null ? undefined : { maxHeight }}
        >
          {view === "root" ? (
            <div className="nx-model-menu__root" role="group" aria-label="Model and effort">
              <button
                ref={modelRow}
                type="button"
                className="nx-model-menu__row"
                data-testid="composer-model-row"
                aria-label={`Model: ${current}`}
                onClick={() => {
                  setQuery("");
                  setActive(null);
                  setView("models");
                }}
              >
                <span className="nx-model-menu__row-title">Model</span>
                <span className="nx-model-menu__row-value">{current}</span>
                <RowChevron />
              </button>
              {variants.length > 0 ? (
                <button
                  ref={effortRow}
                  type="button"
                  className="nx-model-menu__row"
                  data-testid="composer-effort-row"
                  aria-label={`Effort: ${variant ? capitalize(variant) : ""}`}
                  onClick={() => setView("effort")}
                >
                  <span className="nx-model-menu__row-title">Effort</span>
                  <span className="nx-model-menu__row-value">
                    {variant ? capitalize(variant) : ""}
                  </span>
                  <RowChevron />
                </button>
              ) : null}
            </div>
          ) : (
            <button
              type="button"
              className="nx-model-menu__back"
              data-testid="composer-menu-back"
              aria-label="Back"
              onClick={() => {
                focusRowAfterBack.current = view === "effort" ? "effort" : "model";
                setView("root");
              }}
            >
              <span aria-hidden="true">‹</span>
              <span>{view === "effort" ? "Effort" : "Model"}</span>
            </button>
          )}
          {view === "models" ? (
            <>
          <div className="nx-model-menu__search">
            <input
              ref={search}
              type="search"
              aria-controls={visible.length > 0 ? `${id}-list` : undefined}
              aria-activedescendant={highlighted ? optionId(highlighted) : undefined}
              aria-autocomplete="list"
              aria-label="Search models"
              placeholder="Search models…"
              autoComplete="off"
              spellCheck={false}
              data-testid="composer-model-search"
              value={query}
              onChange={(event) => {
                setQuery(event.target.value);
                setActive(null);
              }}
              onKeyDown={onSearchKeyDown}
            />
          </div>
          <div
            className="nx-model-menu__scroll"
            ref={list}
            role="region"
            aria-label="Models"
            tabIndex={0}
            data-testid="model-picker-scroll"
          >
            {visible.length === 0 ? (
              <p className="nx-model-menu__empty" role="status">
                {`No models match “${trimmed}”.`}
              </p>
            ) : (
              <div
                id={`${id}-list`}
                role="listbox"
                aria-label="Model list"
                className="nx-model-menu__list"
              >
                {groups.map((group) => (
                  <div
                    key={group.id}
                    className="nx-model-menu__section"
                    role="group"
                    aria-label={group.title}
                  >
                    <div className="nx-model-menu__heading" aria-hidden="true">
                      <span className="nx-model-menu__heading-title">{group.title}</span>
                      <span className="nx-model-menu__badge">{tierBadge(group.tier)}</span>
                      {group.tier === "included" ? null : (
                        <span className="nx-model-menu__count">{group.models.length}</span>
                      )}
                    </div>
                    {group.models.map((candidate) => {
                      const selected = candidate.id === model?.id;
                      const sub = modelSubId(candidate);
                      return (
                        <div
                          key={candidate.id}
                          id={optionId(candidate.id)}
                          role="option"
                          aria-selected={selected}
                          className={`nx-model-menu__item${
                            candidate.id === highlighted ? " is-active" : ""
                          }${selected ? " is-selected" : ""}`}
                          onPointerMove={() => {
                            if (candidate.id !== highlighted) setActive(candidate.id);
                          }}
                          onClick={() => choose(candidate.id)}
                        >
                          <span className="nx-menu__icon" aria-hidden="true">
                            {selected ? "✓" : ""}
                          </span>
                          <span className="nx-model-menu__name">
                            <span className="nx-model-menu__label">{candidate.label}</span>
                            {sub ? <span className="nx-model-menu__sub">{sub}</span> : null}
                          </span>
                          <span className="nx-model-menu__badge">
                            {tierBadge(candidate.tier ?? "byok")}
                          </span>
                        </div>
                      );
                    })}
                  </div>
                ))}
              </div>
            )}
          </div>
            </>
          ) : null}
          {view === "effort" && variants.length > 0 ? (
            <div
              className="nx-model-menu__effort"
              role="radiogroup"
              aria-label="Reasoning effort"
              data-testid="reasoning-effort"
            >
              {variants.map((name) => (
                <button
                  key={name}
                  type="button"
                  role="radio"
                  aria-checked={variant === name}
                  className={`nx-effort${variant === name ? " is-selected" : ""}`}
                  data-testid={`reasoning-${name}`}
                  onClick={() => {
                    onSelectVariant?.(name);
                    focusRowAfterBack.current = "effort";
                    setView("root");
                  }}
                >
                  <span className="nx-menu__icon" aria-hidden="true">
                    {variant === name ? "✓" : ""}
                  </span>
                  {capitalize(name)}
                </button>
              ))}
            </div>
          ) : null}
          {view === "models" ? (
            <>
          <div className="nx-model-menu__footer">
            <button
              type="button"
              className="nx-menu__item nx-model-menu__action"
              data-testid="composer-manage-models"
              disabled={!onManageModels}
              onClick={() => {
                setOpen(false);
                onManageModels?.();
              }}
            >
              <span className="nx-menu__text">Manage models</span>
            </button>
            <button
              type="button"
              className="nx-menu__item nx-model-menu__action"
              data-testid="composer-connect-provider"
              disabled={!onConnectProvider}
              onClick={() => {
                setOpen(false);
                onConnectProvider?.();
              }}
            >
              <span className="nx-menu__icon" aria-hidden="true">
                ＋
              </span>
              <span className="nx-menu__text">Connect another provider…</span>
            </button>
          </div>
            </>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}

function RowChevron(): ReactNode {
  return (
    <svg
      className="nx-model-menu__row-chevron"
      width="10"
      height="10"
      viewBox="0 0 10 10"
      aria-hidden="true"
      focusable="false"
    >
      <path
        d="M3.5 2l3 3-3 3"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function Chevron(): ReactNode {
  return (
    <svg
      className="nx-model-picker__chevron"
      width="10"
      height="10"
      viewBox="0 0 10 10"
      aria-hidden="true"
      focusable="false"
    >
      <path
        d="M2 4l3 3 3-3"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
