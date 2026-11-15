import type { AsifLoss } from "../../bridge/wire";
import { Badge } from "../../components";
import {
  asifLossCountMismatch,
  type AsifDirection,
} from "../../domain/session";

export interface AsifLossListProps {
  direction: AsifDirection;
  lossCount: number;
  losses: AsifLoss[];
}

/**
 * The full declared-loss list. The message text is path-free by contract, but it is
 * still rendered as text and never interpreted or copied into a link.
 */
export function AsifLossList({ direction, lossCount, losses }: AsifLossListProps) {
  const mismatch = asifLossCountMismatch(lossCount, losses);
  return (
    <section
      className="asif-report"
      aria-label={`${direction === "export" ? "Export" : "Import"} losses`}
      data-testid={`${direction}-losses`}
    >
      <h4 className="section-title">
        Declared losses <Badge tone={lossCount > 0 ? "warn" : "ok"}>{lossCount}</Badge>
      </h4>
      {mismatch ? (
        <p className="notice notice--warn" role="status" data-testid={`${direction}-loss-count-mismatch`}>
          VOWP declared {lossCount} loss{lossCount === 1 ? "" : "es"}, but this summary
          contains {losses.length}. Both numbers are shown; nothing is hidden.
        </p>
      ) : null}
      {losses.length === 0 ? (
        <p className="muted small">VOWP declared no losses for this package.</p>
      ) : (
        <ul className="asif-list">
          {losses.map((loss, index) => (
            <li
              key={`${index}:${loss.kind}`}
              className="asif-list__item"
              data-testid={`${direction}-loss-${index}`}
            >
              <div className="asif-list__head">
                <Badge tone="warn" mono>
                  {loss.kind}
                </Badge>
                <span className="muted small mono">
                  {loss.stage}
                  {loss.scope ? ` · ${loss.scope}` : ""}
                </span>
              </div>
              <span className="small">{loss.message}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

export interface AsifWarningsProps {
  warnings: string[];
}

/** Reindex warnings are not losses; they are shown in their own list. */
export function AsifWarnings({ warnings }: AsifWarningsProps) {
  return (
    <section className="asif-report" aria-label="Import warnings" data-testid="import-warnings">
      <h4 className="section-title">
        Reindex warnings <Badge tone={warnings.length > 0 ? "info" : "unknown"}>{warnings.length}</Badge>
      </h4>
      {warnings.length === 0 ? (
        <p className="muted small">VOWP reported no reindex warnings.</p>
      ) : (
        <ul className="asif-list">
          {warnings.map((warning, index) => (
            <li
              key={`${index}:${warning}`}
              className="asif-list__item asif-list__item--warning"
              data-testid={`import-warning-${index}`}
            >
              <span className="small">{warning}</span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

export interface AsifUnsupportedProps {
  direction: AsifDirection;
  unsupported: string[];
}

/** VOWP operations the package does not carry. They are never silently dropped. */
export function AsifUnsupported({ direction, unsupported }: AsifUnsupportedProps) {
  return (
    <section
      className="asif-report"
      aria-label="Unsupported VOWP operations"
      data-testid={`${direction}-unsupported`}
    >
      <h4 className="section-title">
        Unsupported VOWP operations{" "}
        <Badge tone={unsupported.length > 0 ? "warn" : "unknown"}>{unsupported.length}</Badge>
      </h4>
      {unsupported.length === 0 ? (
        <p className="muted small">VOWP declared no unsupported operations.</p>
      ) : (
        <>
          <ul className="asif-list asif-list--inline">
            {unsupported.map((feature, index) => (
              <li
                key={`${index}:${feature}`}
                className="asif-list__item asif-list__item--inline"
                data-testid={`${direction}-unsupported-${index}`}
              >
                <Badge tone="warn" mono>
                  {feature}
                </Badge>
              </li>
            ))}
          </ul>
          <p className="muted small">
            These operations are not carried in the package. A required unsupported
            feature is refused by VOWP; StackSpirit never forces it.
          </p>
        </>
      )}
    </section>
  );
}
