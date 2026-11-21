import {
  SPRITE_EYES,
  SPRITE_H,
  SPRITE_LAYERS,
  SPRITE_PALETTE,
  SPRITE_W,
} from "./spiritSprite";

function Layer({ name }: { name: string }) {
  const layer = SPRITE_LAYERS[name] ?? {};
  return (
    <>
      {Object.entries(layer).map(([key, d]) => (
        <path key={key} d={d} fill={SPRITE_PALETTE[key]} />
      ))}
    </>
  );
}

/**
 * The StackSpirit loading indicator: the exact pixel-art sprout (design/brand/mark/grid.json)
 * drawn as SVG, with a CSS loop. The two-leaf sprout grows, unfurls and sways out of the
 * head and the eyes blink. Under `prefers-reduced-motion` it is a static calm pose.
 */
export function SpiritLoader({
  label = "StackSpirit is thinking",
  size = 28,
  showLabel = false,
}: {
  label?: string;
  size?: number;
  showLabel?: boolean;
}) {
  const height = Math.round((size * SPRITE_H) / SPRITE_W);
  return (
    <span className="nx-loader" role="status" data-testid="spirit-loader">
      <svg
        className="nx-loader__svg"
        viewBox={`0 0 ${SPRITE_W} ${SPRITE_H}`}
        width={size}
        height={height}
        shapeRendering="crispEdges"
        aria-hidden="true"
        focusable="false"
      >
        <g className="nx-loader__sprout">
          <Layer name="stem" />
          <g className="nx-loader__leaf nx-loader__leaf--left">
            <Layer name="leafLeft" />
          </g>
          <g className="nx-loader__leaf nx-loader__leaf--right">
            <Layer name="leafRight" />
          </g>
        </g>
        <Layer name="body" />
        {SPRITE_EYES.map((box, index) => (
          <g
            key={index}
            className="nx-loader__eye"
            style={{
              transformOrigin: `${(box.left + box.right) / 2}px ${(box.top + box.bottom) / 2}px`,
            }}
          >
            <Layer name={`eye${index}`} />
          </g>
        ))}
      </svg>
      {showLabel ? (
        <span className="nx-loader__label" aria-hidden="true">
          {label}
        </span>
      ) : null}
      <span className="nx-sr-only">{label}</span>
    </span>
  );
}
