import type { WorkerCardView } from "../../bridge/wire";
import { Badge } from "../../components";
import { executorBadges } from "../../domain/executor";
import { eventAgeLabel, formatAgeParts, shortId } from "../../domain/format";

export function WorkerCard({
  card,
  title = null,
  showProject = false,
}: {
  card: WorkerCardView;
  title?: string | null;
  showProject?: boolean;
}) {
  const badges = executorBadges(card.executor);
  const stalled = card.stalled || card.state === "stalled";

  return (
    <article
      className={`worker-card${stalled ? " worker-card--stalled" : ""}`}
      data-testid={`worker-card-${card.job_id}`}
      data-state={stalled ? "stalled" : card.state}
      data-shell={badges.shell ? "true" : "false"}
    >
      <header className="worker-card__head">
        <span className="mono worker-card__job">{card.job_id}</span>
        <Badge tone="info">{card.kind}</Badge>
        <span className="worker-card__age">{formatAgeParts(card.age_secs)}</span>
      </header>

      <p
        className="worker-card__title"
        title={title ?? undefined}
        data-testid={`worker-title-${card.job_id}`}
      >
        {title ?? card.job_id}
        {title ? null : (
          <span
            className="muted small worker-card__untitled"
            title="Title unavailable in snapshot"
          >
            {" "}
            · title unavailable in snapshot
          </span>
        )}
      </p>

      <div className="worker-card__executor" title={badges.raw || undefined}>
        <span data-testid="executor-harness">
          <Badge tone={badges.shell ? "unknown" : "accent"}>{badges.harness}</Badge>
        </span>
        {badges.model ? (
          <span data-testid="executor-model">
            <Badge tone={badges.unknown ? "danger" : "info"}>
              {badges.provider ? `${badges.provider} · ` : ""}
              {badges.model}
            </Badge>
          </span>
        ) : null}
        {badges.modifier ? (
          <span data-testid="executor-modifier">
            <Badge tone="verifier">{badges.modifier}</Badge>
          </span>
        ) : null}
      </div>

      <footer className="worker-card__foot">
        <span
          className={`worker-card__event${stalled ? " worker-card__event--stalled" : ""}`}
          data-testid={`worker-event-${card.job_id}`}
        >
          {eventAgeLabel(card.last_event_age_secs, stalled)}
        </span>
        {showProject ? <span className="muted small">{card.project}</span> : null}
        {card.session_id ? (
          <span className="muted small mono" title={card.session_id}>
            {shortId(card.session_id, 10)}
          </span>
        ) : null}
      </footer>
    </article>
  );
}
