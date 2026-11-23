import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties } from "react";

import {
  COMPACT_JOB_THRESHOLD,
  COMPACT_WIDTH,
  KIND_LABEL,
  STATE_LABEL,
  layoutDag,
  type DagLayout,
  type DagNode,
} from "./dagMap";
import type { ProjectJob } from "./summary";

interface CanvasProps {
  layout: DagLayout;
  compact: boolean;
  selectedId?: string | null;
  onSelect: (node: DagNode) => void;
  /** Suppresses selection right after a drag (whiteboard panning). */
  shouldIgnoreClick?: () => boolean;
}

/** The nodes and Bézier connectors of one laid-out DAG. Shared by the embedded map and the whiteboard. */
export function DagCanvas({ layout, compact, selectedId, onSelect, shouldIgnoreClick }: CanvasProps) {
  return (
    <div
      className={`nx-dag${compact ? " nx-dag--compact" : ""}`}
      data-testid="dag-canvas"
      data-compact={compact ? "true" : "false"}
      style={{ width: layout.width, height: layout.height, "--dag-w": `${layout.nodeW}px`, "--dag-h": `${layout.nodeH}px` } as CSSProperties}
    >
      <svg className="nx-dag__edges" width={layout.width} height={layout.height} aria-hidden="true" focusable="false">
        {layout.edges.map((edge) => (
          <path key={edge.id} d={edge.path} data-edge={edge.id} data-testid="dag-edge" />
        ))}
      </svg>
      {layout.nodes.map((node) => (
        <DagNodeButton
          key={node.id}
          node={node}
          compact={compact}
          selected={selectedId === node.id}
          onClick={() => {
            if (shouldIgnoreClick?.()) return;
            onSelect(node);
          }}
        />
      ))}
    </div>
  );
}

function DagNodeButton({
  node,
  compact,
  selected,
  onClick,
}: {
  node: DagNode;
  compact: boolean;
  selected: boolean;
  onClick: () => void;
}) {
  const kind = KIND_LABEL[node.kind];
  const state = STATE_LABEL[node.state];
  return (
    <button
      type="button"
      className={`nx-dag-node nx-dag-node--${node.kind} nx-dag-node--${node.state} nx-tone-${node.tone}${
        selected ? " nx-dag-node--selected" : ""
      }`}
      style={{ left: node.x, top: node.y }}
      data-testid="dag-node"
      data-node-id={node.id}
      data-kind={node.kind}
      data-state={node.state}
      aria-pressed={selected}
      aria-label={
        node.kind === "root" ? node.label : `${node.label}, ${kind}, ${state}${node.wait ? `, waiting: ${node.wait}` : ""}`
      }
      title={compact ? `${node.label} · ${node.title} · ${kind} · ${state}` : undefined}
      onClick={onClick}
    >
      <span className="nx-dag-node__dot" aria-hidden="true" />
      <span className="nx-dag-node__text">
        <span className="nx-dag-node__top">
          <span className="nx-dag-node__label">{compact ? node.label : node.label}</span>
          {node.kind === "root" ? null : (
            <>
              <span className={`nx-dag-chip nx-dag-chip--${node.kind}`} data-testid="dag-kind-chip">
                {kind}
              </span>
              {compact ? null : (
                <span className={`nx-dag-chip nx-dag-chip--state-${node.state}`} data-testid="dag-state-chip">
                  {state}
                </span>
              )}
            </>
          )}
        </span>
        {compact ? null : (
          <>
            {node.kind === "root" ? null : <span className="nx-dag-node__title">{node.title}</span>}
            <span className="nx-dag-node__sub">{node.sub}</span>
          </>
        )}
      </span>
    </button>
  );
}

/** Compact when the graph is large or the container is narrow. */
export function useDagCompact(jobCount: number, ref: React.RefObject<HTMLElement | null>): boolean {
  const [narrow, setNarrow] = useState(false);
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const measure = () => {
      const width = el.clientWidth;
      setNarrow(width > 0 && width < COMPACT_WIDTH);
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [ref]);
  return jobCount > COMPACT_JOB_THRESHOLD || narrow;
}

/** The embedded Structure map: a true dependency DAG that scrolls rather than clips. */
export function DagMap({
  project,
  rootSub,
  jobs,
  label,
  selectedId,
  onSelectJob,
  onSelectRoot,
}: {
  project: string;
  rootSub: string;
  jobs: ProjectJob[];
  label: string;
  selectedId: string | null;
  onSelectJob: (jobId: string) => void;
  onSelectRoot: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const compact = useDagCompact(jobs.length, ref);
  const layout = useMemo(() => layoutDag(project, rootSub, jobs, compact), [project, rootSub, jobs, compact]);
  return (
    <div className="nx-dag-scroll" ref={ref} role="group" aria-label={label} data-testid="dag-map" tabIndex={0}>
      <DagCanvas
        layout={layout}
        compact={compact}
        selectedId={selectedId}
        onSelect={(node) => (node.kind === "root" ? onSelectRoot() : onSelectJob(node.id))}
      />
    </div>
  );
}

const MIN_ZOOM = 0.2;
const MAX_ZOOM = 2.5;
const clampZoom = (z: number) => Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, z));

/**
 * Full-viewport interactive whiteboard: the detailed DAG on a dotted canvas with
 * click-and-drag panning, zoom controls, wheel zoom and a node detail card.
 */
export function Whiteboard({
  project,
  rootSub,
  jobs,
  onClose,
  onOpenJob,
  onOpenSession,
}: {
  project: string;
  rootSub: string;
  jobs: ProjectJob[];
  onClose: () => void;
  onOpenJob: (jobId: string) => void;
  onOpenSession: () => void;
}) {
  const layout = useMemo(() => layoutDag(project, rootSub, jobs, false), [project, rootSub, jobs]);
  const viewportRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const [view, setView] = useState({ x: 0, y: 0, z: 1 });
  const [selected, setSelected] = useState<DagNode | null>(null);
  const drag = useRef<{ x: number; y: number; vx: number; vy: number; moved: boolean } | null>(null);
  const justDragged = useRef(false);
  const viewRef = useRef(view);
  useEffect(() => {
    viewRef.current = view;
  }, [view]);

  const fit = () => {
    const rect = viewportRef.current?.getBoundingClientRect();
    if (!rect || rect.width === 0 || rect.height === 0) {
      setView({ x: 0, y: 0, z: 1 });
      return;
    }
    const z = clampZoom(Math.min((rect.width - 48) / layout.width, (rect.height - 48) / layout.height, 1.25));
    setView({ z, x: (rect.width - layout.width * z) / 2, y: (rect.height - layout.height * z) / 2 });
  };
  const zoomBy = (factor: number, cx?: number, cy?: number) => {
    const rect = viewportRef.current?.getBoundingClientRect();
    const px = cx ?? (rect ? rect.width / 2 : 0);
    const py = cy ?? (rect ? rect.height / 2 : 0);
    setView((v) => {
      const z = clampZoom(v.z * factor);
      const k = z / v.z;
      return { z, x: px - (px - v.x) * k, y: py - (py - v.y) * k };
    });
  };

  // Open fitted, focus the Close button, and restore focus on close.
  useEffect(() => {
    const opener = document.activeElement as HTMLElement | null;
    fit();
    closeRef.current?.focus();
    return () => opener?.focus?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.stopPropagation();
        onClose();
      }
    };
    document.addEventListener("keydown", onKey, true);
    return () => document.removeEventListener("keydown", onKey, true);
  }, [onClose]);

  // A non-passive wheel listener so the page behind never scrolls.
  useEffect(() => {
    const el = viewportRef.current;
    if (!el) return;
    const onWheel = (event: WheelEvent) => {
      event.preventDefault();
      const rect = el.getBoundingClientRect();
      zoomBy(Math.exp(-event.deltaY * (event.ctrlKey ? 0.01 : 0.0015)), event.clientX - rect.left, event.clientY - rect.top);
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, []);

  const onPointerDown = (event: React.PointerEvent) => {
    if (event.button !== 0) return;
    drag.current = { x: event.clientX, y: event.clientY, vx: viewRef.current.x, vy: viewRef.current.y, moved: false };
    const move = (e: PointerEvent) => {
      const d = drag.current;
      if (!d) return;
      const dx = e.clientX - d.x;
      const dy = e.clientY - d.y;
      if (!d.moved && Math.hypot(dx, dy) > 4) d.moved = true;
      if (d.moved) setView((v) => ({ ...v, x: d.vx + dx, y: d.vy + dy }));
    };
    const up = () => {
      justDragged.current = drag.current?.moved ?? false;
      drag.current = null;
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      // The click that follows pointerup is swallowed only when this gesture was a pan.
      setTimeout(() => (justDragged.current = false), 0);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  };

  const job = selected && selected.kind !== "root" ? jobs.find((item) => item.job.job_id === selected.id) : null;

  return (
    <div className="nx-wb" role="dialog" aria-modal="true" aria-label={`${project} structure whiteboard`} data-testid="whiteboard">
      <div className="nx-wb__bar">
        <h2 className="nx-wb__title">{project} · Structure</h2>
        <div className="nx-wb__tools" role="group" aria-label="Zoom">
          <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" aria-label="Zoom out" onClick={() => zoomBy(1 / 1.25)}>
            −
          </button>
          <button
            type="button"
            className="nx-btn nx-btn--quiet nx-btn--sm"
            aria-label="Reset zoom to 100%"
            data-testid="whiteboard-zoom"
            onClick={() => setView((v) => ({ ...v, z: 1 }))}
          >
            {Math.round(view.z * 100)}%
          </button>
          <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" aria-label="Zoom in" onClick={() => zoomBy(1.25)}>
            +
          </button>
          <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" onClick={fit}>
            Fit view
          </button>
        </div>
        <button type="button" ref={closeRef} className="nx-btn nx-btn--sm" onClick={onClose}>
          Close
        </button>
      </div>
      <div className="nx-wb__viewport" ref={viewportRef} onPointerDown={onPointerDown} data-testid="whiteboard-viewport">
        <div
          className="nx-wb__stage"
          data-testid="whiteboard-stage"
          style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.z})` }}
        >
          <DagCanvas
            layout={layout}
            compact={false}
            selectedId={selected?.id ?? null}
            onSelect={setSelected}
            shouldIgnoreClick={() => justDragged.current}
          />
        </div>
        {selected ? (
          <aside className="nx-wb__card" data-testid="whiteboard-card" aria-label={`${selected.label} details`} onPointerDown={(e) => e.stopPropagation()}>
            <div className="nx-wb__card-head">
              <strong>{selected.label}</strong>
              <button type="button" className="nx-btn nx-btn--quiet nx-btn--sm" aria-label="Dismiss details" onClick={() => setSelected(null)}>
                ×
              </button>
            </div>
            {selected.kind === "root" ? null : <p className="nx-wb__card-title">{selected.title}</p>}
            <dl className="nx-wb__facts">
              {selected.kind === "root" ? null : (
                <>
                  <dt>Kind</dt>
                  <dd>{KIND_LABEL[selected.kind]}</dd>
                  <dt>State</dt>
                  <dd>{STATE_LABEL[selected.state]}</dd>
                </>
              )}
              {selected.model ? (
                <>
                  <dt>Model</dt>
                  <dd>{selected.model}</dd>
                </>
              ) : null}
              {selected.wait ? (
                <>
                  <dt>Waiting</dt>
                  <dd>{selected.wait}</dd>
                </>
              ) : null}
              {selected.needs.length > 0 ? (
                <>
                  <dt>Needs</dt>
                  <dd>{selected.needs.join(", ")}</dd>
                </>
              ) : null}
              {job && job.job.reason && !selected.wait ? (
                <>
                  <dt>Reason</dt>
                  <dd>{job.job.reason}</dd>
                </>
              ) : null}
            </dl>
            <button
              type="button"
              className="nx-btn nx-btn--sm"
              onClick={() => {
                onClose();
                if (selected.kind === "root") onOpenSession();
                else onOpenJob(selected.id);
              }}
            >
              {selected.kind === "root" ? "Open orchestrator" : "Open job details"}
            </button>
          </aside>
        ) : null}
        <p className="nx-wb__hint">Drag to pan · scroll to zoom · Esc to close</p>
      </div>
    </div>
  );
}
