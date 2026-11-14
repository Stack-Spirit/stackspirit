import { Badge, Button, Drawer, EmptyState } from "../components";
import { formatAge, formatClock } from "../domain/format";
import { useFleet, useFleetStore, useRawDocument } from "../state/fleetStore";

export function InboxDrawer({ open, onClose }: { open: boolean; onClose: () => void }) {
  const fleet = useFleet();
  const items = fleet?.inbox.filter((item) => item.state === "open") ?? [];
  return (
    <Drawer open={open} title="Inbox" onClose={onClose}>
      {items.length === 0 ? (
        <EmptyState
          icon="◔"
          title="Inbox is clear"
          body="Questions and review requests from VOWP appear here."
        />
      ) : (
        <ul className="inbox-list">
          {items.map((item) => (
            <li key={item.id} className="inbox-list__item">
              <div className="inbox-list__head">
                <Badge tone={item.kind === "question" ? "info" : "unknown"}>{item.kind}</Badge>
                <span className="muted small">{formatAge(item.age_secs)} ago</span>
              </div>
              <strong>{item.title}</strong>
              <span className="muted small mono">
                {item.project ?? "all"} · {item.job_id ?? "no job"}
              </span>
            </li>
          ))}
        </ul>
      )}
      <p className="muted small">
        The full inbox drawer with answer actions arrives with the Fleet Canvas job (D3).
      </p>
    </Drawer>
  );
}

export function RawDocumentDrawer({ open, onClose }: { open: boolean; onClose: () => void }) {
  const raw = useRawDocument();
  const fleet = useFleet();
  const reload = useFleetStore((state) => state.loadRawDocument);

  const warnings = [...(raw?.status_warnings ?? []), ...(raw?.capacity_warnings ?? [])];

  return (
    <Drawer
      open={open}
      title="Raw VOWP document"
      onClose={onClose}
      footer={
        <div className="inline-actions">
          <Badge tone={warnings.length > 0 ? "warn" : "ok"}>
            {warnings.length} parse warning{warnings.length === 1 ? "" : "s"}
          </Badge>
          <Button variant="secondary" size="sm" onClick={() => void reload()}>
            Reload
          </Button>
        </div>
      }
    >
      <p className="muted small">
        Read-only and redacted by core. Raw VOWP field names are shown here for diagnostics;
        for example, a renamed field may produce a warning while the view keeps the last
        good data.
      </p>
      <dl className="raw-meta">
        <div>
          <dt>generated_at</dt>
          <dd className="mono">{raw?.generated_at ?? formatClock(fleet?.generated_at)}</dd>
        </div>
        <div>
          <dt>fetched_at</dt>
          <dd className="mono">{raw?.fetched_at ?? formatClock(fleet?.fetched_at)}</dd>
        </div>
        <div>
          <dt>source</dt>
          <dd className="mono">{fleet?.source ?? "unknown"}</dd>
        </div>
      </dl>

      <section aria-label="Parse warnings">
        <h3 className="section-title">Parse warnings</h3>
        {warnings.length === 0 ? (
          <p className="muted small">No warnings were produced by the last parse.</p>
        ) : (
          <ul className="warning-list">
            {warnings.map((warning, index) => (
              <li key={`${warning.path}-${index}`} className="warning-list__item">
                <Badge tone={warning.code === "truncated" ? "warn" : "info"}>
                  {warning.code}
                </Badge>
                <span className="mono small">{warning.path}</span>
                <span className="muted small">{warning.detail}</span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section aria-label="Parsed fleet status">
        <h3 className="section-title">Parsed fleet status</h3>
        <pre className="raw-json">{raw?.status ? JSON.stringify(raw.status, null, 2) : "No parsed document."}</pre>
      </section>

      <section aria-label="Parsed capacity report">
        <h3 className="section-title">Parsed capacity report</h3>
        <pre className="raw-json">
          {raw?.capacity ? JSON.stringify(raw.capacity, null, 2) : "No parsed document."}
        </pre>
      </section>
    </Drawer>
  );
}
