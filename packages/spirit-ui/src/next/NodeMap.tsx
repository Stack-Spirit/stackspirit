import type { NodeTone } from "./summary";

export interface MapNode {
  id: string;
  label: string;
  /** One short line under the label. */
  sub?: string;
  tone: NodeTone;
  onSelect?: () => void;
  selected?: boolean;
}

export interface MapBranch {
  node: MapNode;
  leaves: MapNode[];
  /** Shown as a "+N more" chip after the leaves. */
  more?: number;
  onMore?: () => void;
}

/**
 * A left-to-right structure map: one root, a column of branches, and each branch's
 * leaves. Connector lines are drawn in CSS, so it needs no layout pass and reflows
 * with the window.
 */
export function NodeMap({ root, branches, label }: { root: MapNode; branches: MapBranch[]; label: string }) {
  return (
    <div className="nx-map" role="group" aria-label={label}>
      <div className="nx-map__root">
        <Node node={root} />
      </div>
      {branches.length > 0 ? (
        <div className="nx-map__branches">
          {branches.map((branch) => (
            <div key={branch.node.id} className="nx-map__branch">
              <Node node={branch.node} />
              {branch.leaves.length > 0 || branch.more ? (
                <div className="nx-map__leaves">
                  {branch.leaves.map((leaf) => (
                    <Node key={leaf.id} node={leaf} small />
                  ))}
                  {branch.more ? (
                    <button type="button" className="nx-node nx-node--small nx-node--more" onClick={branch.onMore}>
                      +{branch.more} more
                    </button>
                  ) : null}
                </div>
              ) : null}
            </div>
          ))}
        </div>
      ) : null}
    </div>
  );
}

function Node({ node, small = false }: { node: MapNode; small?: boolean }) {
  const className = `nx-node nx-node--${node.tone}${small ? " nx-node--small" : ""}${
    node.selected ? " nx-node--selected" : ""
  }`;
  const body = (
    <>
      <span className="nx-node__dot" aria-hidden="true" />
      <span className="nx-node__text">
        <span className="nx-node__label">{node.label}</span>
        {node.sub ? <span className="nx-node__sub">{node.sub}</span> : null}
      </span>
    </>
  );
  return node.onSelect ? (
    <button type="button" className={className} onClick={node.onSelect} aria-pressed={node.selected}>
      {body}
    </button>
  ) : (
    <div className={className}>{body}</div>
  );
}
