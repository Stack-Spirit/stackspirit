import { Badge, Button, Drawer } from "../../components";
import { formatClock, formatDateTime } from "../../domain/format";
import { allWarnings, prettyParsed, warningCountLabel, warningTone } from "../../domain/raw";
import { useFleet, useFleetStore, useRawDocument } from "../../state/fleetStore";

export function RawDocumentDrawer({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const raw = useRawDocument();
  const fleet = useFleet();
  const reload = useFleetStore((state) => state.loadRawDocument);
  const warnings = allWarnings(raw);

  return (
    <Drawer
      open={open}
      title="Raw VOWP document"
      onClose={onClose}
      footer={
        <div className="inline-actions">
          <Badge tone={warnings.length > 0 ? "warn" : "ok"}>
            {warningCountLabel(warnings.length)}
          </Badge>
          <Button variant="secondary" size="sm" onClick={() => void reload()}>
            Reload
          </Button>
        </div>
      }
    >
      <div className="raw-document" data-testid="raw-document-drawer">
        <p className="muted small">
          Read-only and redacted by core. Only parsed fields and parse warnings are shown;
          raw command lines, environment values and stderr are never displayed here.
        </p>

        <dl className="raw-meta">
          <div>
            <dt>generated_at</dt>
            <dd className="mono" title={raw?.generated_at ?? undefined}>
              {raw?.generated_at ?? formatClock(fleet?.generated_at)}
            </dd>
          </div>
          <div>
            <dt>fetched_at</dt>
            <dd className="mono" title={raw?.fetched_at ?? undefined}>
              {raw?.fetched_at ?? formatClock(fleet?.fetched_at)}
            </dd>
          </div>
          <div>
            <dt>source</dt>
            <dd className="mono">{fleet?.source ?? "unknown"}</dd>
          </div>
          <div>
            <dt>loaded</dt>
            <dd className="mono">{raw ? formatDateTime(raw.fetched_at) : "never"}</dd>
          </div>
        </dl>

        <section aria-label="Parse warnings">
          <h3 className="section-title">Parse warnings</h3>
          {warnings.length === 0 ? (
            <p className="muted small">
              No parse warnings were produced by the last parse. This is not a promise
              that VOWP will not add fields later.
            </p>
          ) : (
            <ul className="warning-list">
              {warnings.map((warning, index) => (
                <li
                  key={`${warning.path}-${warning.code}-${index}`}
                  className="warning-list__item"
                  data-testid="raw-warning"
                >
                  <Badge tone={warningTone(warning.code)}>{warning.code}</Badge>
                  <span className="mono small">{warning.path}</span>
                  <span className="muted small">{warning.detail}</span>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section aria-label="Parsed fleet status">
          <h3 className="section-title">Parsed fleet status</h3>
          <pre className="raw-json" tabIndex={0} data-testid="parsed-fleet-status">
            {prettyParsed(raw?.status)}
          </pre>
        </section>

        <section aria-label="Parsed capacity report">
          <h3 className="section-title">Parsed capacity report</h3>
          <pre className="raw-json" tabIndex={0} data-testid="parsed-capacity-report">
            {prettyParsed(raw?.capacity)}
          </pre>
        </section>
      </div>
    </Drawer>
  );
}
