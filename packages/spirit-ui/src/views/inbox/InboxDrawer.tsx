import { useEffect, useState } from "react";

import type { InboxItemView } from "../../bridge/wire";
import { toSpiritError } from "../../bridge/errors";
import { Badge, Drawer, EmptyState } from "../../components";
import { formatAgeParts } from "../../domain/format";
import { useFleet, useFleetStore, useMutationsAllowed } from "../../state/fleetStore";
import { InboxItem } from "./InboxItem";
import "./inbox.css";

type InboxTab = "open" | "all";

export function InboxDrawer({ open, onClose }: { open: boolean; onClose: () => void }) {
  const bridge = useFleetStore((state) => state.bridge);
  const refresh = useFleetStore((state) => state.refresh);
  const fleet = useFleet();
  const mutationsAllowed = useMutationsAllowed();
  const [items, setItems] = useState<InboxItemView[]>([]);
  const [tab, setTab] = useState<InboxTab>("open");
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [loaded, setLoaded] = useState(false);
  const [loading, setLoading] = useState(true);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    if (!bridge) return;
    let cancelled = false;
    void bridge.fleet
      .inbox(true)
      .then((entries) => {
        if (cancelled) return;
        setItems(entries);
        setLoaded(true);
        setSelectedId((selected) => selected ?? entries.find((item) => item.state === "open")?.id ?? entries[0]?.id ?? null);
      })
      .catch((caught: unknown) => {
        if (!cancelled) setError(toSpiritError(caught).message);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [open, bridge]);

  // Before the full-item read resolves, show the snapshot summaries rather than an
  // empty inbox. Once it resolves, its result is authoritative even when empty: an
  // empty full read must not fall back to stale snapshot items.
  const source = loaded ? items : (fleet?.inbox ?? []);
  const visible = tab === "open" ? source.filter((item) => item.state === "open") : source;
  const selected = visible.find((item) => item.id === selectedId) ?? visible[0] ?? null;
  const openCount = source.filter((item) => item.state === "open").length;

  async function answer(item: InboxItemView, text: string): Promise<void> {
    if (!bridge) {
      setError("The bridge is not initialized.");
      return;
    }
    setBusyId(item.id);
    setError(null);
    try {
      const answered = await bridge.fleet.answerInbox(item.id, text);
      setItems((current) =>
        current.map((candidate) => (candidate.id === answered.id ? answered : candidate)),
      );
      setTab("all");
      setSelectedId(answered.id);
      await refresh();
    } catch (caught) {
      setError(toSpiritError(caught).message);
    } finally {
      setBusyId(null);
    }
  }

  return (
    <Drawer open={open} title="Inbox" onClose={onClose}>
      <div className="inbox-drawer" data-testid="inbox-drawer">
        <div className="inbox-drawer__tabs" role="group" aria-label="Inbox filter">
          <button
            type="button"
            className={`inbox-drawer__tab${tab === "open" ? " inbox-drawer__tab--active" : ""}`}
            aria-pressed={tab === "open"}
            onClick={() => setTab("open")}
          >
            Open <Badge tone="info">{openCount}</Badge>
          </button>
          <button
            type="button"
            className={`inbox-drawer__tab${tab === "all" ? " inbox-drawer__tab--active" : ""}`}
            aria-pressed={tab === "all"}
            onClick={() => setTab("all")}
          >
            All <Badge tone="unknown">{source.length}</Badge>
          </button>
          {loading ? <span className="muted small">Loading full items…</span> : null}
        </div>

        {error ? (
          <p className="notice notice--danger" role="alert">
            {error}
          </p>
        ) : null}
        {!mutationsAllowed ? (
          <p className="notice notice--warn" role="status">
            Refresh before answering. The last snapshot is stale or the host is not
            connected.
          </p>
        ) : null}

        {visible.length === 0 ? (
          <EmptyState
            icon="◔"
            title={tab === "open" ? "Inbox is clear" : "No inbox items"}
            body={
              tab === "open"
                ? "Questions and review requests from VOWP appear here."
                : "VOWP has not returned any inbox items."
            }
          />
        ) : (
          <div className="inbox-drawer__layout">
            <ul className="inbox-list" aria-label="Inbox items">
              {visible.map((item) => (
                <li key={item.id}>
                  <button
                    type="button"
                    className={`inbox-list__item inbox-list__button${
                      selected?.id === item.id ? " inbox-list__button--selected" : ""
                    }`}
                    aria-current={selected?.id === item.id ? "true" : undefined}
                    onClick={() => setSelectedId(item.id)}
                  >
                    <span className="inbox-list__head">
                      <Badge tone={item.kind === "question" ? "info" : "unknown"}>
                        {item.kind}
                      </Badge>
                      <span className="muted small">{formatAgeParts(item.age_secs)} ago</span>
                    </span>
                    <strong>{item.title}</strong>
                    <span className="muted small mono">
                      {item.project ?? "all"} · {item.job_id ?? "no job"}
                    </span>
                  </button>
                </li>
              ))}
            </ul>

            {selected ? (
              <InboxItem
                key={selected.id}
                item={selected}
                busy={busyId === selected.id}
                disabled={!mutationsAllowed}
                disabledReason="Refresh before answering."
                onAnswer={(text) => void answer(selected, text)}
              />
            ) : null}
          </div>
        )}
      </div>
    </Drawer>
  );
}
