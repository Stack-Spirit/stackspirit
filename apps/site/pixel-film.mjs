/**
 * Procedural pixel-art sprite film engine for StackSpirit.
 *
 * Draws the skinny, deadpan frog-eyed sprout spirit (inspired directly by
 * design/brand/gen/r2-body.png and design/brand/mark/stackspirit-mark-master.png)
 * purely in code, pixel by pixel, frame by frame — no static sprite PNGs or
 * video codec hacks required. Works identically in Safari, Chrome, and Firefox.
 *
 * Virtual canvas: 22 x 34 logical pixels, rendered at integer pixel scale
 * with nearest-neighbor crispness.
 */

const PALETTE = {
  K: "#0d120f", // deep outline & frog pupils
  D: "#1e5b36", // deep pine shadow / far limb
  M: "#2e7f48", // mid green shade
  G: "#43a85a", // primary Meadow green
  L: "#7ccb72", // upper-left lit green
  S: "#cdedb0", // bright sprout rim highlight
  W: "#f5f7f0", // eye sparkle
};

function px(ctx, x, y, ch, scale) {
  const color = PALETTE[ch];
  if (!color) return;
  ctx.fillStyle = color;
  ctx.fillRect(Math.round(x) * scale, Math.round(y) * scale, scale, scale);
}

/** Paint a row-encoded pixel sprite matrix anchored at (ox, oy). */
function blit(ctx, ox, oy, rows, scale) {
  for (let r = 0; r < rows.length; r += 1) {
    const row = rows[r];
    for (let c = 0; c < row.length; c += 1) {
      const ch = row[c];
      if (ch !== ".") px(ctx, ox + c, oy + r, ch, scale);
    }
  }
}

/** Sprout variants (sway -1, 0, +1 px lag behind stride). */
const SPROUT_FRAMES = [
  [
    "..KKSK...KKSK..",
    ".KSSLGK.KGLSSK.",
    ".KGGMDK.KDGMMK.",
    "..KKKKGKGKKKK..",
    "......KGK......",
  ],
  [
    "...KKSK...KKSK.",
    "..KSSLGK.KGLSSK",
    "..KGGMDK.KDGMMK",
    "...KKKKGKGKKKK.",
    ".......KGK.....",
  ],
  [
    ".KKSK...KKSK...",
    "KSSLGK.KGLSSK..",
    "KGGMDK.KDGMMK..",
    ".KKKKGKGKKKK...",
    ".....KGK.......",
  ],
];

/** Iconic deadpan frog-eyed head (14x11) — open eyes vs blink. */
const HEAD_OPEN = [
  "..KKK....KKK..",
  ".KSLGK..KSLGK.",
  "KSKKWKGGKSKKWK",
  "KSKKKGGGGKKKMK",
  "KLGKKGGLGGKKDK",
  "KLLGGGGGGGGMDK",
  "KLGGGGGGGGGGDK",
  "KLGGGGKKKKGGDK",
  ".KGGGGGGGGMDK.",
  "..KGGGGGMMDK..",
  "...KKKKKKKK...",
];

const HEAD_BLINK = [
  "..KKK....KKK..",
  ".KSLGK..KSLGK.",
  "KSLGGKGGKSLGGK",
  "KLKKKKGGKKKKMK",
  "KLGGGGGLGGGGDK",
  "KLLGGGGGGGGMDK",
  "KLGGGGGGGGGGDK",
  "KLGGGGKKKKGGDK",
  ".KGGGGGGGGMDK.",
  "..KGGGGGMMDK..",
  "...KKKKKKKK...",
];

/** Slim upright torso (8x10) modeled after design/brand/gen/r2-body.png (skinny!). */
const TORSO = [
  ".KKKKKK.",
  "KLLGGGDK",
  "KLGGGGDK",
  "KLGGGGDK",
  "KLGGGGDK",
  "KLGGGMDK",
  "KLGGGMDK",
  "KMGGGMDK",
  ".KGGMDK.",
  ".KKKKKK.",
];

/**
 * Draw an articulated slender limb segment with 1px dark outline and Meadow fill.
 * Used for both legs and arms across all 8 walk frames.
 */
function drawLimb(ctx, pts, fill, rim, scale) {
  // Outline pass (4-neighbor expansion around each joint pixel)
  for (const [x, y] of pts) {
    px(ctx, x - 1, y, "K", scale);
    px(ctx, x + 1, y, "K", scale);
    px(ctx, x, y - 1, "K", scale);
    px(ctx, x, y + 1, "K", scale);
  }
  // Fill pass
  for (let i = 0; i < pts.length; i += 1) {
    const [x, y] = pts[i];
    px(ctx, x, y, i === 0 && rim ? rim : fill, scale);
  }
}

function sampleSegment(x0, y0, x1, y1) {
  const pts = [];
  const steps = Math.max(1, Math.round(Math.max(Math.abs(x1 - x0), Math.abs(y1 - y0))));
  for (let i = 0; i <= steps; i += 1) {
    const t = i / steps;
    pts.push([Math.round(x0 + (x1 - x0) * t), Math.round(y0 + (y1 - y0) * t)]);
  }
  return pts;
}

/**
 * 8-frame walk-cycle kinematics for the skinny frog-eyed spirit.
 * Each frame defines hip->knee->foot and shoulder->hand offsets relative to torso.
 */
const WALK_POSES = [
  // 0: Contact right forward, left back
  { bob: 0, sprout: 0, nearFoot: [3, 9], nearKnee: [2, 5], farFoot: [-3, 9], farKnee: [-1, 5], nearHand: [-3, 7], farHand: [3, 6] },
  // 1: Down / recoil
  { bob: 1, sprout: 2, nearFoot: [2, 9], nearKnee: [2, 5], farFoot: [-2, 8], farKnee: [-1, 5], nearHand: [-2, 7], farHand: [2, 6] },
  // 2: Passing right planted, left lifting
  { bob: 0, sprout: 0, nearFoot: [0, 9], nearKnee: [0, 5], farFoot: [0, 7], farKnee: [2, 4], nearHand: [0, 7], farHand: [0, 6] },
  // 3: High-point / push-off
  { bob: -1, sprout: 1, nearFoot: [-2, 9], nearKnee: [-1, 5], farFoot: [2, 8], farKnee: [2, 4], nearHand: [2, 6], farHand: [-2, 7] },
  // 4: Contact left forward, right back
  { bob: 0, sprout: 0, nearFoot: [-3, 9], nearKnee: [-1, 5], farFoot: [3, 9], farKnee: [2, 5], nearHand: [3, 6], farHand: [-3, 7] },
  // 5: Down / recoil
  { bob: 1, sprout: 2, nearFoot: [-2, 8], nearKnee: [-1, 5], farFoot: [2, 9], farKnee: [2, 5], nearHand: [2, 6], farHand: [-2, 7] },
  // 6: Passing left planted, right lifting
  { bob: 0, sprout: 0, nearFoot: [0, 7], nearKnee: [2, 4], farFoot: [0, 9], farKnee: [0, 5], nearHand: [0, 6], farHand: [0, 7] },
  // 7: High-point / push-off
  { bob: -1, sprout: 1, nearFoot: [2, 8], nearKnee: [2, 4], farFoot: [-2, 9], farKnee: [-1, 5], nearHand: [-2, 7], farHand: [2, 6] },
];

export const WALKER_LOGICAL_W = 22;
export const WALKER_LOGICAL_H = 34;

/**
 * Render one crisp frame of the skinny deadpan spirit walking (or standing idle when poseIndex === -1).
 */
export function renderWalkerFrame(ctx, poseIndex, { blink = false, scale = 2, facingLeft = false } = {}) {
  const w = WALKER_LOGICAL_W * scale;
  const h = WALKER_LOGICAL_H * scale;
  ctx.clearRect(0, 0, w, h);
  ctx.save();
  ctx.imageSmoothingEnabled = false;
  if (facingLeft) {
    ctx.translate(w, 0);
    ctx.scale(-1, 1);
  }

  const idlePose = {
    bob: 0,
    sprout: 0,
    nearFoot: [2, 9],
    nearKnee: [2, 5],
    farFoot: [-2, 9],
    farKnee: [-2, 5],
    nearHand: [4, 7],
    farHand: [-4, 7],
  };
  const pose = poseIndex < 0 ? idlePose : WALK_POSES[poseIndex % WALK_POSES.length];

  const cx = 11;
  const headY = 4 + pose.bob;
  const torsoX = cx - 4;
  const torsoY = headY + 10;
  const hipY = torsoY + 8;
  const shoulderY = torsoY + 2;

  // 1. Far arm (behind torso, shaded pine D)
  const farArmPts = sampleSegment(cx - 2, shoulderY, cx + pose.farHand[0], shoulderY + pose.farHand[1]);
  drawLimb(ctx, farArmPts, "D", "M", scale);

  // 2. Far leg + little frog foot (behind torso, shaded D/M)
  const farLegPts = [
    ...sampleSegment(cx - 1, hipY, cx + pose.farKnee[0], hipY + pose.farKnee[1]),
    ...sampleSegment(cx + pose.farKnee[0], hipY + pose.farKnee[1], cx + pose.farFoot[0], hipY + pose.farFoot[1]),
    [cx + pose.farFoot[0] + 1, hipY + pose.farFoot[1]],
  ];
  drawLimb(ctx, farLegPts, "D", "M", scale);

  // 3. Slim upright torso
  blit(ctx, torsoX, torsoY, TORSO, scale);

  // 4. Near leg + little frog foot (front, primary Meadow green G with L rim)
  const nearLegPts = [
    ...sampleSegment(cx + 1, hipY, cx + pose.nearKnee[0], hipY + pose.nearKnee[1]),
    ...sampleSegment(cx + pose.nearKnee[0], hipY + pose.nearKnee[1], cx + pose.nearFoot[0], hipY + pose.nearFoot[1]),
    [cx + pose.nearFoot[0] + 1, hipY + pose.nearFoot[1]],
  ];
  drawLimb(ctx, nearLegPts, "G", "L", scale);

  // 5. Near slender arm (front, lit Meadow green G/L)
  const nearArmPts = sampleSegment(cx + 1, shoulderY, cx + pose.nearHand[0], shoulderY + pose.nearHand[1]);
  drawLimb(ctx, nearArmPts, "G", "L", scale);

  // 6. Big deadpan frog-eyed head
  blit(ctx, cx - 7, headY, blink ? HEAD_BLINK : HEAD_OPEN, scale);

  // 7. Tiny 2-leaf sprout on top
  blit(ctx, cx - 8, headY - 4, SPROUT_FRAMES[pose.sprout], scale);

  ctx.restore();
}

export const ICON_LOGICAL_W = 16;
export const ICON_LOGICAL_H = 16;

/**
 * Render the code-generated animated deadpan sprout head icon (for header / callouts).
 */
export function renderSproutIcon(ctx, { blink = false, sway = 0, scale = 2 } = {}) {
  const w = ICON_LOGICAL_W * scale;
  const h = ICON_LOGICAL_H * scale;
  ctx.clearRect(0, 0, w, h);
  ctx.save();
  ctx.imageSmoothingEnabled = false;
  blit(ctx, 1, 5, blink ? HEAD_BLINK : HEAD_OPEN, scale);
  blit(ctx, 0, 1, SPROUT_FRAMES[sway % SPROUT_FRAMES.length], scale);
  ctx.restore();
}
