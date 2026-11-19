import type { ReactNode } from "react";

import { SpiritMark } from "./SpiritMark";

export interface EmptyHeroProps {
  /** True while the session has no user or model turn yet. */
  active: boolean;
  /** Calm title: StackSpirit for setup, or the session's own title. */
  title: string;
  /** One short line of context; never a password prompt. */
  lede: string;
  /** Clickable prompt chips shown under the composer. */
  suggestions: string[];
  disabled: boolean;
  onPick: (text: string) => void;
  /** A compact saved-server or workspace chip, when one applies. */
  above?: ReactNode;
  /** The composer card and its dock row. Kept in this wrapper in both layouts. */
  children: ReactNode;
}

/**
 * The empty-session hero geometry from the brief: a vertically and horizontally
 * centred stack with the spirit mark and a 26/32 title, the composer in the middle,
 * and prompt chips below.
 *
 * The same wrapper always surrounds the composer. When the first turn arrives it
 * becomes `nx-hero--docked`; the composer is not remounted, and the Message field
 * regains focus when the turn leaves the session ready again.
 */
export function EmptyHero({
  active,
  title,
  lede,
  suggestions,
  disabled,
  onPick,
  above,
  children,
}: EmptyHeroProps) {
  return (
    <div className={`nx-hero${active ? "" : " nx-hero--docked"}`}>
      {active ? (
        <div className="nx-hero__head">
          <div className="nx-hero__headline">
            <SpiritMark size={34} />
            <h2 className="nx-hero__title">{title}</h2>
          </div>
          {lede ? <p className="nx-hero__lede">{lede}</p> : null}
        </div>
      ) : null}
      {active ? above : null}
      {children}
      {active && suggestions.length > 0 ? (
        <div className="nx-suggest nx-hero__chips">
          {suggestions.map((text) => (
            <button key={text} type="button" disabled={disabled} onClick={() => onPick(text)}>
              {text}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );
}
