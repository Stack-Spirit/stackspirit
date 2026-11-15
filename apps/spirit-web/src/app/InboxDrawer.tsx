import { useEffect, useState } from "react";

import type { InboxItemView, SpiritBridge, SpiritError, WebFleetView } from "@stackspirit/ui";
import { Badge, Drawer, EmptyState } from "@stackspirit/ui";

import { READ_ONLY_NOTICE } from "../copy";
import { asSpiritError } from "../bridge/errors";
import { formatAgeParts, formatDateTime } from "@stackspirit/ui";

export function WebInboxDrawer({
  open,
  onClose,
  bridge,
  snapshot,
}: {
  open: boolean;
  onClose: () => void;
  bridge: SpiritBridge;
  snapshot: WebFleetView | null;
}) {
  const [items, setItems] = useState<InboxItemView[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<SpiritError | null>(null);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    void Promise.resolve().then(async () => {
      if (cancelled) return;
      setLoading(true);
      setError(null);
      try {
        const entries = await bridge.fleet.inbox(true);
        if (!cancelled) {
          setItems(entries);
          setLoaded(true);
        }
      } catch (caught) {
        if (!cancelled) setError(asSpiritError(caught));
      } finally {
        if (!cancelled) setLoading(false);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [open, bridge]);

  const source = loaded ? items : (snapshot?.inbox ?? []);
  const openCount = source.filter((item) => item.state === "open").length;

  return (
    <Drawer open={open} title="Inbox" onClose={onClose}>
      <div className="inbox-drawer" data-testid="web-inbox-drawer">
        <p className="notice notice--warn" role="status">
          {READ_ONLY_NOTICE} This drawer never sends an answer.
        </p>
        <div className="inbox-drawer__tabs" role="group" aria-label="Inbox summary">
          <Badge tone="info">board {snapshot?.board.inbox_open ?? "unknown"} open</Badge>
          <Badge tone="unknown">{openCount} open loaded</Badge>
          <Badge tone="unknown">{source.length} items loaded</Badge>
          {loading ? <span className="muted small">Loading full items…</span> : null}
        </div>
        {error ? (
          <p className="notice notice--danger" role="alert">
            {error.message}
          </p>
        ) : null}
        {source.length === 0 ? (
          <EmptyState
            icon="◔"
            title="Inbox is clear"
            body="VOWP did not return any inbox items for this daemon."
          />
        ) : (
          <ul className="web-list web-inbox-list" aria-label="Inbox items">
            {source.map((item) => (
              <li key={item.id} className="web-inbox-item" data-testid={`inbox-item-${item.id}`}>
                <div className="web-inbox-item__head">
                  <Badge tone={item.kind === "question" ? "info" : "unknown"}>{item.kind}</Badge>
                  <Badge tone="unknown">{item.state}</Badge>
                  <span className="muted small">
                    age {formatAgeParts(item.age_secs)} · {formatDateTime(item.created_at)}
                  </span>
                </div>
                <h3>{item.title}</h3>
                {item.body ? (
                  <p className="web-inbox-item__body" data-testid="inbox-body">
                    {item.body}
                  </p>
                ) : (
                  <p className="muted small">
                    The snapshot has no full body. The all-items read did not provide one.
                  </p>
                )}
                {item.options.length > 0 ? (
                  <div className="web-inbox-item__options" aria-label="Suggested answers">
                    {item.options.map((option) => (
                      <Badge key={option} tone="info">
                        {option}
                      </Badge>
                    ))}
                  </div>
                ) : null}
                {item.project || item.job_id ? (
                  <p className="muted small mono">
                    {item.project ?? "all"} · {item.job_id ?? "no job"}
                  </p>
                ) : null}
                {item.state === "answered" ? (
                  <p className="muted small" data-testid="inbox-answered">
                    Answered by {item.answered_by ?? "unknown"}
                    {item.answered_at ? ` at ${formatDateTime(item.answered_at)}` : ""}:{" "}
                    {item.answer ?? "No answer recorded."}
                  </p>
                ) : null}
              </li>
            ))}
          </ul>
        )}
      </div>
    </Drawer>
  );
}
