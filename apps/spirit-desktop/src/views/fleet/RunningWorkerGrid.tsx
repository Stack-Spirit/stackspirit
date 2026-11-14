import type { FleetView, WorkerCardView } from "../../bridge/wire";
import { EmptyState } from "../../components";
import { findJobTitle, sortWorkerCards } from "../../domain/fleet";
import { WorkerCard } from "./WorkerCard";

export function RunningWorkerGrid({
  fleet,
  cards = fleet.running,
  showProject = false,
}: {
  fleet: FleetView;
  cards?: readonly WorkerCardView[];
  showProject?: boolean;
}) {
  const sorted = sortWorkerCards(cards);
  return (
    <section className="running-grid" aria-label="Running workers">
      <header className="running-grid__head">
        <span className="eyebrow">Running workers</span>
        <span className="muted small">{cards.length} visible</span>
      </header>
      {cards.length === 0 ? (
        <EmptyState
          icon="○"
          title="No running workers"
          body="VOWP has no running worker cards in this snapshot."
        />
      ) : (
        <div className="running-grid__cards">
          {sorted.map((card) => (
            <WorkerCard
              key={`${card.project}:${card.job_id}`}
              card={card}
              title={findJobTitle(fleet, card.job_id)}
              showProject={showProject}
            />
          ))}
        </div>
      )}
    </section>
  );
}
