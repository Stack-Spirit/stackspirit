import markUrl from "./assets/spirit-mark.png";

/**
 * The StackSpirit mark: the exact master artwork (design/brand/mark). It is shown
 * with smooth scaling at UI sizes, like any logo; the hand-drawn 16 and 32 px grids
 * are for favicons and app icons, where pixels map one to one.
 */
export function SpiritMark({ size = 28, label }: { size?: number; label?: string }) {
  return (
    <img
      className="nx-mark"
      src={markUrl}
      width={size}
      height={size}
      alt={label ?? ""}
      aria-hidden={label ? undefined : true}
      draggable={false}
    />
  );
}
