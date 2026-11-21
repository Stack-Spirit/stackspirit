import {
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";

import type { ChatModelView, ChatUsageView, SessionId } from "../bridge/wire";
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

function tierLabel(model: ChatModelView): string {
  if (model.tier === "included") return "Included";
  if (model.tier === "byok") return "BYOK";
  if (model.tier === "orchestrator") return "Orchestrator";
  return model.provider;
}

interface ChatModelGroup {
  id: "included" | "connected";
  title: string;
  models: ChatModelView[];
}

/**
 * Groups the verified chat model list the way the composer shows it: the bundled
 * free models first under "OpenCode Free", then every provider-owned choice under
 * "Connected providers". Empty groups are omitted, never rendered as a blank heading.
 */
export function groupChatModels(models: ChatModelView[]): ChatModelGroup[] {
  const included = models.filter((model) => model.tier === "included");
  const connected = models.filter((model) => model.tier !== "included");
  return [
    ...(included.length > 0
      ? [{ id: "included" as const, title: "OpenCode Free", models: included }]
      : []),
    ...(connected.length > 0
      ? [{ id: "connected" as const, title: "Connected providers", models: connected }]
      : []),
  ];
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
  onSend: (text: string) => void | Promise<void>;
  onStop: () => void;
  onStart: () => void;
  onSelectModel?: (modelId: string) => void;
  /** Reveals the app-owned secure server card; never opens it on its own. */
  onConnectServer?: () => void;
  showConnectServer?: boolean;
  onOpenEngineSettings?: () => void;
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
}: ComposerBarProps) {
  const [text, setText] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [providerSettingsOpen, setProviderSettingsOpen] = useState(false);
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

  async function submit() {
    if (disabled || streaming || sending || submitting) return;
    const trimmed = text.trim();
    if (!trimmed) return;
    setText("");
    const pending = onSend(trimmed);
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
        className={`nx-composer${disabled ? " nx-composer--disabled" : ""}`}
        aria-busy={sending}
        onSubmit={(event) => {
          event.preventDefault();
          void submit();
        }}
      >
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
      />
      <div className="nx-composer__controls">
        <div className="nx-composer__left">
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

/** Case-insensitive match over label, id, provider and tier. Every term must hit. */
export function filterChatModels(models: ChatModelView[], query: string): ChatModelView[] {
  const terms = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (terms.length === 0) return models;
  return models.filter((model) => {
    const haystack = `${model.label} ${model.id} ${model.provider} ${tierLabel(model)}`.toLowerCase();
    return terms.every((term) => haystack.includes(term));
  });
}

/** Provider-owned models grouped by provider (popular order is the engine's order). */
function providerSections(models: ChatModelView[]) {
  const sections: { id: string; title: string; badge: string; models: ChatModelView[] }[] = [];
  const free = models.filter((model) => model.tier === "included");
  if (free.length > 0) {
    sections.push({ id: "included", title: "OpenCode Free", badge: "Free", models: free });
  }
  for (const model of models) {
    if (model.tier === "included") continue;
    let section = sections.find((entry) => entry.id === `p:${model.provider}`);
    if (!section) {
      section = {
        id: `p:${model.provider}`,
        title: model.provider,
        badge: "Connected",
        models: [],
      };
      sections.push(section);
    }
    section.models.push(model);
  }
  return sections;
}

function ModelPicker({
  model,
  models,
  disabled,
  openRequest = 0,
  onSelect,
  onConnectProvider,
  onManageModels,
}: {
  model: ChatModelView | null;
  models: ChatModelView[];
  disabled: boolean;
  openRequest?: number;
  onSelect?: (modelId: string) => void;
  onConnectProvider?: () => void;
  onManageModels?: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const id = useId();
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const search = useRef<HTMLInputElement>(null);
  /** A recovery notice opens the picker with focus left on the trigger (ss-76 contract). */
  const keepTriggerFocus = useRef(false);

  const optionId = (index: number) => `${id}-opt-${index}`;
  const visible = filterChatModels(models, query);
  const sections = providerSections(visible);
  const flat = sections.flatMap((section) => section.models);
  const indexOf = new Map(flat.map((candidate, index) => [candidate.id, index]));
  const activeIndex = Math.min(active, Math.max(flat.length - 1, 0));

  const close = (restoreFocus: boolean) => {
    setOpen(false);
    setQuery("");
    setActive(0);
    if (restoreFocus) trigger.current?.focus();
  };

  useEffect(() => {
    if (openRequest <= 0) return;
    // A recovery notice asks for the picker after its click handler; defer one tick so
    // the popover can measure the trigger before it opens.
    const timer = setTimeout(() => {
      keepTriggerFocus.current = true;
      setOpen(true);
      trigger.current?.focus();
    }, 0);
    return () => clearTimeout(timer);
  }, [openRequest]);

  useEffect(() => {
    if (!open) return;
    if (keepTriggerFocus.current) keepTriggerFocus.current = false;
    else search.current?.focus();
    const onPointer = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) close(false);
    };
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") close(true);
    };
    document.addEventListener("pointerdown", onPointer);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("pointerdown", onPointer);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  useEffect(() => {
    if (!open) return;
    document.getElementById(optionId(activeIndex))?.scrollIntoView?.({ block: "nearest" });
    // eslint-disable-next-line react-hooks/exhaustive-deps -- optionId only closes over `id`
  }, [open, activeIndex, query, id]);

  const choose = (candidate: ChatModelView) => {
    close(true);
    onSelect?.(candidate.id);
  };

  const onSearchKey = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      if (flat.length > 0) setActive((activeIndex + 1) % flat.length);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      if (flat.length > 0) setActive((activeIndex - 1 + flat.length) % flat.length);
    } else if (event.key === "Enter") {
      event.preventDefault();
      const candidate = flat[activeIndex];
      if (candidate) choose(candidate);
    }
  };

  const current = model ? modelDisplayLabel(model) : "Choose a model";

  return (
    <div className="nx-menu nx-model-picker" ref={root}>
      <button
        ref={trigger}
        type="button"
        className="nx-model-picker__trigger"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={id}
        aria-label={`Model: ${current}`}
        data-testid="composer-model"
        title={current}
        disabled={disabled || !onSelect}
        onClick={() => (open ? close(false) : setOpen(true))}
      >
        <span className="nx-model-picker__label">{current}</span>
        <Chevron />
      </button>
      {open ? (
        <div
          id={id}
          className="nx-menu__pop nx-menu__pop--above nx-menu__pop--start nx-model-menu"
          data-testid="composer-model-popover"
        >
          <input
            ref={search}
            type="search"
            role="searchbox"
            className="nx-model-menu__search"
            data-testid="composer-model-search"
            placeholder="Search models"
            aria-label="Search models"
            aria-controls={`${id}-list`}
            aria-activedescendant={flat.length > 0 ? optionId(activeIndex) : undefined}
            autoComplete="off"
            spellCheck={false}
            value={query}
            onChange={(event) => {
              setQuery(event.target.value);
              setActive(0);
            }}
            onKeyDown={onSearchKey}
          />
          <div
            id={`${id}-list`}
            className="nx-model-menu__list"
            data-testid="composer-model-list"
            role="menu"
            aria-label="Choose a model"
          >
            {sections.map((section) => (
              <div
                key={section.id}
                className="nx-model-menu__section"
                role="group"
                aria-label={section.title}
              >
                <span className="nx-model-menu__heading">
                  <span aria-hidden="true">{section.title}</span>
                  <span className="nx-model-menu__badge" data-testid="model-section-badge">
                    {section.badge}
                  </span>
                </span>
                {section.models.map((candidate) => {
                  const index = indexOf.get(candidate.id) ?? 0;
                  return (
                    <button
                      key={candidate.id}
                      id={optionId(index)}
                      type="button"
                      tabIndex={-1}
                      role="menuitemradio"
                      aria-checked={candidate.id === model?.id}
                      data-active={index === activeIndex ? "true" : undefined}
                      className="nx-menu__item nx-model-menu__item"
                      onMouseMove={() => setActive(index)}
                      onClick={() => choose(candidate)}
                    >
                      <span className="nx-menu__icon" aria-hidden="true">
                        {candidate.id === model?.id ? "✓" : ""}
                      </span>
                      <span className="nx-menu__text">
                        {modelDisplayLabel(candidate)}
                        <span className="nx-model-menu__id" aria-hidden="true">
                          {candidate.id}
                        </span>
                      </span>
                      <span className="nx-menu__meta">{tierLabel(candidate)}</span>
                    </button>
                  );
                })}
              </div>
            ))}
            {flat.length === 0 ? (
              <p className="nx-model-menu__empty" role="status">
                No models match “{query}”.
              </p>
            ) : null}
          </div>
          <div className="nx-model-menu__footer">
            <button
              type="button"
              className="nx-menu__item nx-model-menu__manage"
              data-testid="composer-manage-models"
              onClick={() => {
                close(false);
                onManageModels?.();
              }}
              disabled={!onManageModels}
            >
              <span className="nx-menu__text">Manage models</span>
            </button>
            <button
              type="button"
              className="nx-menu__item nx-model-menu__connect"
              data-testid="composer-connect-provider"
              onClick={() => {
                close(false);
                onConnectProvider?.();
              }}
              disabled={!onConnectProvider}
            >
              <span className="nx-menu__icon" aria-hidden="true">
                ＋
              </span>
              <span className="nx-menu__text">Connect another provider…</span>
            </button>
          </div>
        </div>
      ) : null}
    </div>
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
