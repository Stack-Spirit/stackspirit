import {
  useEffect,
  useId,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";

import type { ChatModelView, ChatUsageView, SessionId } from "../bridge/wire";
import { ContextMeter, UsageStats } from "./ContextMeter";

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
  const textarea = useRef<HTMLTextAreaElement>(null);
  /** True while the Message field owns (or is about to regain) focus. */
  const restoreFocus = useRef(false);
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
              disabled={disabled}
              onSelect={onSelectModel}
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
              onClick={onStop}
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
  disabled,
  onSelect,
}: {
  model: ChatModelView | null;
  models: ChatModelView[];
  disabled: boolean;
  onSelect?: (modelId: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const id = useId();
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    const onPointer = (event: PointerEvent) => {
      if (!root.current?.contains(event.target as Node)) setOpen(false);
    };
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key === "Escape") {
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
  }, [open]);

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
        onClick={() => setOpen((value) => !value)}
      >
        <span className="nx-model-picker__label">{current}</span>
        <Chevron />
      </button>
      {open ? (
        <div
          id={id}
          className="nx-menu__pop nx-menu__pop--above nx-menu__pop--start nx-model-menu"
          role="menu"
          aria-label="Choose a model"
        >
          {models.map((candidate) => (
            <button
              key={candidate.id}
              type="button"
              role="menuitemradio"
              aria-checked={candidate.id === model?.id}
              className="nx-menu__item nx-model-menu__item"
              onClick={() => {
                setOpen(false);
                onSelect?.(candidate.id);
              }}
            >
              <span className="nx-menu__icon" aria-hidden="true">
                {candidate.id === model?.id ? "✓" : ""}
              </span>
              <span className="nx-menu__text">{modelDisplayLabel(candidate)}</span>
              <span className="nx-menu__meta">{tierLabel(candidate)}</span>
            </button>
          ))}
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
