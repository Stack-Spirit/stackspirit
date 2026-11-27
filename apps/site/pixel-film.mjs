/**
 * Procedural pixel-art hopping frog film engine for StackSpirit.
 *
 * Draws our deadpan Meadow pixel-art frog (inspired directly by
 * design/brand/gen/r3-gazer.png and design/brand/mark/stackspirit-mark-master.png)
 * purely in code, frame by frame, hopping smoothly along the rolling hero road.
 *
 * Logical canvas: 26 x 24 pixels, rendered at integer scale with crisp pixels.
 */

const PALETTE = {
  K: "#0d120f", // outline & frog pupils
  D: "#1e5b36", // deep pine shadow
  M: "#2e7f48", // mid green shade
  G: "#43a85a", // primary Meadow green
  L: "#7ccb72", // upper-left lit green
  S: "#cdedb0", // bright sprout highlight
  W: "#f5f7f0", // eye sparkle
};

function px(ctx, x, y, ch, scale) {
  const color = PALETTE[ch];
  if (!color) return;
  ctx.fillStyle = color;
  ctx.fillRect(Math.round(x) * scale, Math.round(y) * scale, scale, scale);
}

function blit(ctx, ox, oy, rows, scale) {
  for (let r = 0; r < rows.length; r += 1) {
    const row = rows[r];
    for (let c = 0; c < row.length; c += 1) {
      const ch = row[c];
      if (ch !== ".") px(ctx, ox + c, oy + r, ch, scale);
    }
  }
}

/** Little 2-leaf sprout on top of the frog's head */
const SPROUT = [
  "..KKSK..KKSK..",
  ".KSSLGKKGLSSK.",
  "..KKKKGKGKKK..",
  "......KGK.....",
];

/** Frame 0: Seated deadpan frog (calm idle / pause between hops) */
const FROG_SIT_OPEN = [
  "......KKK....KKK......",
  ".....KSLGK..KSLGK.....",
  "....KSKKWKGGKSKKWK....",
  "....KSKKKGGGGKKKMK....",
  "...KLLGKKGGGGKKGDK....",
  "..KLLGGGGGGGGGGGGDK...",
  ".KLGGGGGGGGGKKKKGDK...",
  ".KLGGGGGGGGGGGGGGMDK..",
  "KLLGGGGGGGGGGGGGGMDK..",
  "KLGGGDGGGGGGGGGGGMDK..",
  "KLGGDMGGGGGGGGGGMMDK..",
  "KMGDDMGGGGGGGGGMMDK...",
  ".KDDDDGGGGGGGGMMDK....",
  ".KKDDKKKGGGGGKKDDK....",
  "..KDDK..KKKKK.KDDKK...",
  "..KKKK........KKKKK...",
];

const FROG_SIT_BLINK = [
  "......KKK....KKK......",
  ".....KSLGK..KSLGK.....",
  "....KSLGGKGGKSLGGK....",
  "....KLKKKKGGKKKKMK....",
  "...KLLGGGGGGGGGGDK....",
  "..KLLGGGGGGGGGGGGDK...",
  ".KLGGGGGGGGGKKKKGDK...",
  ".KLGGGGGGGGGGGGGGMDK..",
  "KLLGGGGGGGGGGGGGGMDK..",
  "KLGGGDGGGGGGGGGGGMDK..",
  "KLGGDMGGGGGGGGGGMMDK..",
  "KMGDDMGGGGGGGGGMMDK...",
  ".KDDDDGGGGGGGGMMDK....",
  ".KKDDKKKGGGGGKKDDK....",
  "..KDDK..KKKKK.KDDKK...",
  "..KKKK........KKKKK...",
];

/** Frame 1: Crouch / anticipation squash right before springing */
const FROG_CROUCH = [
  "......................",
  "......................",
  ".....KKK.....KKK......",
  "....KSLGK...KSLGK.....",
  "...KSKKWKGGGKSKKWK....",
  "...KSKKKGGGGGKKKMK....",
  "..KLLGGGGGGGGGGGGDK...",
  ".KLLGGGGGGGGKKKKGDK...",
  "KLLGGGGGGGGGGGGGGMDK..",
  "KLGGDDGGGGGGGGGGGMDK..",
  "KMGGDDGGGGGGGGGGMMDK..",
  ".KDDDDGGGGGGGGGMMDK...",
  ".KKDDDKKGGGGGGKKDDKK..",
  "..KKKKK.KKKKKK.KKKKK..",
];

/** Frame 2: Launch / rising leap — back legs kicked out behind */
const FROG_LEAP_UP = [
  ".........KKK....KKK...",
  "........KSLGK..KSLGK..",
  ".......KSKKWKGGKSKKWK.",
  ".......KSKKKGGGGKKKMK.",
  "......KLLGGGGGGGGGGDK.",
  ".....KLLGGGGGGGKKKKDK.",
  "....KLLGGGGGGGGGGGGDK.",
  "...KLLGGGGGGGGGGGGMDK.",
  "..KDMGGGGGGGGGGGGMDK..",
  ".KDDK.KGGGGGGGGMMDK...",
  "KDDK...KKGGGGKKKDK....",
  "KKK......KKKK...KK....",
];

/** Frame 3: Mid-air apex stretch — sleek horizontal glide */
const FROG_AIR = [
  "..........KKK....KKK..",
  ".........KSLGK..KSLGK.",
  "........KSKKWKGGKSKKWK",
  "........KSKKKGGGGKKKMK",
  "......KKLLGGGGGGGGGGDK",
  "....KKLLGGGGGGGGKKKKDK",
  "..KKLGGGGGGGGGGGGGGMDK",
  ".KDMGGGGGGGGGGGGGGMDK.",
  "KDDK..KKGGGGGGGGKKK...",
  "KKK.....KKKKKKKK..KK..",
];

/** Frame 4: Descending / reaching front feet down to land */
const FROG_LAND_REACH = [
  ".......KKK....KKK.....",
  "......KSLGK..KSLGK....",
  ".....KSKKWKGGKSKKWK...",
  ".....KSKKKGGGGKKKMK...",
  "....KLLGGGGGGGGGGDK...",
  "...KLLGGGGGGKKKKGDK...",
  "..KLLGGGGGGGGGGGGMDK..",
  ".KLMGGGGGGGGGGGGMMDK..",
  ".KDDKGGGGGGGGGGKMDK...",
  "..KDDK.KKGGGGK..KDK...",
  "...KK....KKKK...KKK...",
];

export const FROG_LOGICAL_W = 26;
export const FROG_LOGICAL_H = 24;

/**
 * Render one frame of our code-generated hopping frog.
 * phase: "sit" | "crouch" | "up" | "air" | "down"
 */
export function renderFrogHopFrame(ctx, phase, { blink = false, scale = 2, facingLeft = false } = {}) {
  const w = FROG_LOGICAL_W * scale;
  const h = FROG_LOGICAL_H * scale;
  ctx.clearRect(0, 0, w, h);
  ctx.save();
  ctx.imageSmoothingEnabled = false;
  if (facingLeft) {
    ctx.translate(w, 0);
    ctx.scale(-1, 1);
  }

  let sprite = blink ? FROG_SIT_BLINK : FROG_SIT_OPEN;
  let ox = 2;
  let oy = 7;
  let sproutX = 5;
  let sproutY = 3;

  if (phase === "crouch") {
    sprite = FROG_CROUCH;
    ox = 2;
    oy = 9;
    sproutX = 5;
    sproutY = 7;
  } else if (phase === "up") {
    sprite = FROG_LEAP_UP;
    ox = 2;
    oy = 6;
    sproutX = 8;
    sproutY = 2;
  } else if (phase === "air") {
    sprite = FROG_AIR;
    ox = 2;
    oy = 7;
    sproutX = 9;
    sproutY = 3;
  } else if (phase === "down") {
    sprite = FROG_LAND_REACH;
    ox = 2;
    oy = 7;
    sproutX = 6;
    sproutY = 3;
  }

  blit(ctx, ox, oy, sprite, scale);
  blit(ctx, sproutX, sproutY, SPROUT, scale);
  ctx.restore();
}
