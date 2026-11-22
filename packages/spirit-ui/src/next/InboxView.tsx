import { useEffect, useState } from "react";

import { toSpiritError } from "../bridge/errors";
import type { InboxItemView } from "../bridge/wire";
import { formatAgeParts } from "../domain/format";
import { useFleet, useFleetStore, useMutationsAllowed } from "../state/fleetStore";
import { openInbox } from "./summary";

export type InboxScope = "all" | { project: string };

const MAX_ANSWER_BYTES = 10_000;
const UTF8 = new TextEncoder();

export type InboxKindLabel = "Decision" | "Question" | "Notice";

/** The three kinds the owner sees; any unknown kind is a decision to make. */
export function inboxKindLabel(kind: string): InboxKindLabel {
  const key = kind.toLowerCase();
  if (key === "question") return "Question";
  if (key === "notice" || key === "alert" || key === "info" || key === "warning") return "Notice";
  return "Decision";
}

interface Confirmation {
  id: string;
  title: string;
  answer: string;
}

/**
 * Open inbox items from the fleet snapshot, with one-click option buttons and a
 * free-text answer. `scope="all"` shows Global plus every project with filter
 * chips; `{ project }` shows only that project's items. Answering goes through
 * `bridge.fleet.answerInbox` and then refreshes; the item leaves the list only
 * once the snapshot says it is answered.
 */
export function InboxView({
  scope,
  heading = true,
}: {
  scope: InboxScope;
  /** Render the section heading. The project tab already has its own. */
  heading?: boolean;
}) {
  const fleet = useFleet();
  const bridge = useFleetStore((state) => state.bridge);
  const refresh = useFleetStore((state) => state.refresh);
  const mutationsAllowed = useMutationsAllowed();
  const [filter, setFilter] = useState<string>("all");
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirmations, setConfirmations] = useState<Confirmation[]>([]);

  // The snapshot carries summaries only (no body, no options). Read the full items
  // whenever the set of open ids changes and prefer them over the summaries.
  const [full, setFull] = useState<Map<string, InboxItemView>>(new Map());
  const openKey = openInbox(fleet)
    .map((item) => item.id)
    .join("|");
  useEffect(() => {
    if (!bridge || openKey === "") return;
    let cancelled = false;
    void bridge.fleet
      .inbox(true)
      .then((entries) => {
        if (!cancelled) setFull(new Map(entries.map((entry) => [entry.id, entry])));
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, [bridge, openKey]);

  const project = scope === "all" ? undefined : scope.project;
  const scoped = openInbox(fleet, project).map((item) => {
    const detail = full.get(item.id);
    return detail && detail.state === "open" ? detail : item;
  });
  const projectNames = Array.from(
    new Set(scoped.map((item) => item.project).filter((name): name is string => name !== null)),
  ).sort();
  const globalCount = scoped.filter((item) => item.project === null).length;
  const activeFilter = filter === "all" || filter === "global" || projectNames.includes(filter) ? filter : "all";
  const visible =
    scope !== "all" || activeFilter === "all"
      ? scoped
      : scoped.filter((item) => (activeFilter === "global" ? item.project === null : item.project === activeFilter));
  const label = scope === "all" ? "Inbox" : `${scope.project} inbox`;

  async function answer(item: InboxItemView, text: string): Promise<void> {
    if (!bridge) {
      setError("The bridge is not initialized.");
      return;
    }
    setBusyId(item.id);
    setError(null);
    try {
      const answered = await bridge.fleet.answerInbox(item.id, text);
      setConfirmations((current) => [
        { id: answered.id, title: item.title, answer: text },
        ...current.filter((entry) => entry.id !== answered.id),
      ]);
      await refresh();
    } catch (caught) {
      setError(toSpiritError(caught).message);
    } finally {
      setBusyId(null);
    }
  }

  const chips: Array<{ key: string; label: string; count: number }> = [
    { key: "all", label: "All", count: scoped.length },
    { key: "global", label: "Global", count: globalCount },
    ...projectNames.map((name) => ({
      key: name,
      label: name,
      count: scoped.filter((item) => item.project === name).length,
    })),
  ];

  return (
    <section className="nx-inbox" aria-label={label} data-testid="inbox-view">
      {heading ? (
        <div className="nx-block__head">
          <h2>Inbox</h2>
          <span className="nx-count" data-testid="inbox-open-count" aria-label={`${scoped.length} open`}>
            {scoped.length}
          </span>
        </div>
      ) : null}

      {scope === "all" && scoped.length > 0 ? (
        <div className="nx-segment nx-inbox__chips" role="group" aria-label="Filter inbox">
          {chips.map((chip) => (
            <button
              key={chip.key}
              type="button"
              aria-pressed={activeFilter === chip.key}
              onClick={() => setFilter(chip.key)}
            >
              {chip.label}
              <span className="nx-segment__count">{chip.count}</span>
            </button>
          ))}
        </div>
      ) : null}

      {error ? (
        <p className="nx-banner nx-banner--warn" role="alert">
          {error}
        </p>
      ) : null}
      {!mutationsAllowed && visible.length > 0 ? (
        <p className="nx-banner nx-banner--warn" role="status">
          Refresh before answering. The last snapshot is stale or the server is not connected.
        </p>
      ) : null}
      <div className="nx-inbox__confirmations" role="status" aria-live="polite">
        {confirmations.map((entry) => (
          <p key={entry.id} className="nx-inbox__confirmed" data-testid="inbox-confirmation">
            Answered “{entry.title}” with “{entry.answer}”.
          </p>
        ))}
      </div>

      {visible.length === 0 ? (
        <p className="nx-inbox__empty" data-testid="inbox-empty">
          All clear — nothing waiting for you
        </p>
      ) : (
        <ul className="nx-inbox__list">
          {visible.map((item) => (
            <li key={item.id}>
              <InboxCard
                item={item}
                busy={busyId === item.id}
                disabled={!mutationsAllowed || busyId !== null}
                onAnswer={(text) => void answer(item, text)}
              />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

function InboxCard({
  item,
  busy,
  disabled,
  onAnswer,
}: {
  item: InboxItemView;
  busy: boolean;
  disabled: boolean;
  onAnswer: (answer: string) => void;
}) {
  const [text, setText] = useState("");
  const trimmed = text.trim();
  const bytes = UTF8.encode(trimmed).length;
  const canSend = !disabled && trimmed.length > 0 && bytes <= MAX_ANSWER_BYTES;
  const kind = inboxKindLabel(item.kind);
  const titleId = `inbox-title-${item.id}`;

  return (
    <article className="nx-inbox__item" aria-labelledby={titleId} data-testid="inbox-item" data-project={item.project ?? ""}>
      <header className="nx-inbox__head">
        <span className={`nx-chip nx-chip--tiny nx-inbox__kind nx-inbox__kind--${kind.toLowerCase()}`} data-testid="inbox-kind">
          {kind}
        </span>
        <span className="nx-chip nx-chip--tiny nx-chip--muted" data-testid="inbox-project">
          {item.project ?? "Global"}
        </span>
        <span className="nx-inbox__age">{formatAgeParts(item.age_secs)} old</span>
      </header>
      <h3 className="nx-inbox__title" id={titleId}>
        {item.title}
      </h3>
      {item.body ? <p className="nx-inbox__body">{item.body}</p> : null}
      {item.options.length > 0 ? (
        <div className="nx-inbox__options" role="group" aria-label={`Answers for ${item.title}`}>
          {item.options.map((option) => (
            <button
              key={option}
              type="button"
              className="nx-btn nx-btn--sm"
              disabled={disabled}
              onClick={() => onAnswer(option)}
            >
              {option}
            </button>
          ))}
        </div>
      ) : null}
      <form
        className="nx-inbox__answer"
        onSubmit={(event) => {
          event.preventDefault();
          if (canSend) onAnswer(trimmed);
        }}
      >
        <input
          className="nx-inbox__input"
          type="text"
          value={text}
          placeholder="Or type an answer"
          aria-label={`Answer for ${item.title}`}
          onChange={(event) => setText(event.target.value)}
        />
        <button type="submit" className="nx-btn nx-btn--quiet nx-btn--sm" disabled={!canSend}>
          {busy ? "Sending…" : "Send"}
        </button>
      </form>
    </article>
  );
}
