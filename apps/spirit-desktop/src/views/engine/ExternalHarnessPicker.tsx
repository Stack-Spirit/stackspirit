import { Badge, Button } from "../../components";
import type { ExternalHarness, ExternalHarnessKind } from "../../bridge/wire";
import { externalHarnessLabel } from "../../domain/engine";

export interface ExternalHarnessPickerProps {
  harnesses: ExternalHarness[];
  scanning: boolean;
  selected: boolean;
  error: string | null;
  onScan: () => void;
  onSelect: (kind: ExternalHarnessKind, path: string) => void;
  onUseBundled: () => void;
}

export function ExternalHarnessPicker({
  harnesses,
  scanning,
  selected,
  error,
  onScan,
  onSelect,
  onUseBundled,
}: ExternalHarnessPickerProps) {
  return (
    <div className="engine-section">
      <p className="muted small">
        Use an existing harness if you already have one. Its login and model access stay
        with that installation; StackSpirit's included models apply only to the bundled
        engine.
      </p>
      <div className="engine-actions">
        <Button variant="secondary" size="sm" onClick={onScan} loading={scanning}>
          Scan PATH
        </Button>
        <Button variant={selected ? "primary" : "secondary"} size="sm" onClick={onUseBundled}>
          Use bundled engine
        </Button>
      </div>
      {error ? <p className="engine-error">{error}</p> : null}
      {harnesses.length === 0 ? (
        <p className="muted small">
          No existing harness found. StackSpirit scans only the exact names opencode,
          claude and claude-code on PATH; it does not walk the disk.
        </p>
      ) : (
        <ul className="harness-list" aria-label="Detected harnesses">
          {harnesses.map((harness) => (
            <li key={`${harness.kind}:${harness.path}`} className="harness">
              <span className="engine-model__main">
                <span>
                  <strong>{externalHarnessLabel(harness)}</strong>{" "}
                  {harness.usable ? (
                    <Badge tone="ok">Usable</Badge>
                  ) : (
                    <Badge tone="warn">Not usable</Badge>
                  )}
                </span>
                <span className="mono harness__path" title={harness.path}>
                  {harness.path}
                </span>
                <span className="muted small">
                  Version: {harness.version ?? "not reported"}
                </span>
              </span>
              <Button
                variant="secondary"
                size="sm"
                disabled={!harness.usable}
                title={harness.usable ? undefined : "This installation did not report a version."}
                onClick={() => onSelect(harness.kind, harness.path)}
              >
                Use this installation
              </Button>
            </li>
          ))}
        </ul>
      )}
      <p className="muted small">
        Selecting an external harness never edits its configuration. StackSpirit stops the
        bundled sidecar and reports the external choice; switching back requires the
        explicit button above.
      </p>
    </div>
  );
}
