import { useState } from "react";

import type { InboxItemView } from "../../bridge/wire";
import { Badge, Button } from "../../components";
import { formatAgeParts, formatDateTime } from "../../domain/format";

const MAX_ANSWER = 10_000;

export function InboxItem({
  item,
  onAnswer,
  busy = false,
  disabled = false,
  disabledReason = null,
}: {
  item: InboxItemView;
  onAnswer: (answer: string) => void;
  busy?: boolean;
  disabled?: boolean;
  disabledReason?: string | null;
}) {
  const [answer, setAnswer] = useState("");
  const answered = item.state === "answered";
  const trimmed = answer.trim();
  const canSubmit =
    !answered &&
    !busy &&
    !disabled &&
    trimmed.length > 0 &&
    answer.length <= MAX_ANSWER;

  return (
    <article className="inbox-item" data-testid="inbox-item" data-state={item.state}>
      <header className="inbox-item__head">
        <div className="inbox-item__badges">
          <Badge tone={item.kind === "question" ? "info" : "unknown"}>{item.kind}</Badge>
          {item.project ? <Badge tone="unknown">{item.project}</Badge> : null}
          {item.job_id ? <Badge tone="unknown" mono>{item.job_id}</Badge> : null}
        </div>
        <span className="muted small">{formatAgeParts(item.age_secs)} old</span>
      </header>

      <h3 className="inbox-item__title">{item.title}</h3>

      {item.body ? (
        <p className="inbox-item__body" data-testid="inbox-body">
          {item.body}
        </p>
      ) : (
        <p className="muted small">
          The snapshot has no full body. Open Inbox with the bridge&apos;s all-items read
          to load it.
        </p>
      )}

      {answered ? (
        <div className="inbox-item__answered" data-testid="inbox-answered">
          <Badge tone="ok">answered</Badge>
          <p className="small">{item.answer ?? "No answer recorded."}</p>
          <p className="muted small">
            {item.answered_by ?? "unknown"} · {formatDateTime(item.answered_at)}
          </p>
        </div>
      ) : (
        <div className="inbox-item__answer">
          {item.options.length > 0 ? (
            <div className="inbox-item__options" aria-label="Suggested answers">
              {item.options.map((option) => (
                <Button
                  key={option}
                  variant={answer === option ? "primary" : "secondary"}
                  size="sm"
                  onClick={() => setAnswer(option)}
                >
                  {option}
                </Button>
              ))}
            </div>
          ) : null}
          <label className="field">
            <span className="field__label">Answer</span>
            <textarea
              className="input inbox-item__textarea"
              value={answer}
              maxLength={MAX_ANSWER}
              rows={4}
              placeholder="Type an answer for VOWP…"
              onChange={(event) => setAnswer(event.target.value)}
            />
            <span className="field__hint">
              {answer.length} / {MAX_ANSWER} characters. Empty answers are rejected
              locally; VOWP remains authoritative.
            </span>
          </label>
          <div className="inbox-item__submit">
            <Button
              variant="primary"
              size="sm"
              disabled={!canSubmit}
              onClick={() => onAnswer(trimmed)}
              data-testid="inbox-answer-submit"
            >
              {busy ? "Sending…" : "Send answer"}
            </Button>
            <span className="muted small">
              Answering refreshes the snapshot; the job does not move optimistically.
            </span>
          </div>
          {disabled ? (
            <p className="muted small" data-testid="inbox-disabled-reason">
              {disabledReason ?? "Refresh before answering."}
            </p>
          ) : null}
        </div>
      )}
    </article>
  );
}
