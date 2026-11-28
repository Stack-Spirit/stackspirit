/**
 * Handmade interactions for the StackSpirit marketing site.
 *
 *   1. Authentic ExampleDesign/pixel-canvas shimmer ONLY on card hover (never
 *      noisy dust scattered across the hero background).
 *   2. Code-generated hopping frog film along the North Noir rolling Meadow
 *      curve at the foot of the hero: crouch -> parabolic leap -> land -> blink.
 *   3. Scroll-driven vertical Meadow path on the page edge with the code-gen
 *      frog hopping cleanly as you scroll.
 *
 * `prefers-reduced-motion: reduce` disables animation cleanly.
 */

import {
  FROG_LOGICAL_H,
  FROG_LOGICAL_W,
  renderFrogHopFrame,
} from "./pixel-film.mjs";

const PALETTE_CARD = ["#cdedb0", "#43a85a", "#2e7f48", "#1e5b36"];
const MAX_DPR = 2;

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

function reducedMotionQuery() {
  return window.matchMedia("(prefers-reduced-motion: reduce)");
}

function isBlinking(timeMs) {
  const cycle = timeMs % 3800;
  return cycle > 3620 && cycle < 3760;
}

class ShimmerPixel {
  constructor(x, y, color, speed, maxSize) {
    this.x = x;
    this.y = y;
    this.color = color;
    this.speed = speed;
    this.maxSize = maxSize;
    this.minSize = 0.6;
    this.size = 0;
    this.delay = 0;
    this.counter = 0;
    this.reverse = false;
    this.shimmering = false;
    this.idle = true;
  }

  appear(step) {
    this.idle = false;
    if (this.counter < this.delay) {
      this.counter += step;
      return;
    }
    if (this.size >= this.maxSize) this.shimmering = true;
    if (this.shimmering) {
      if (this.size >= this.maxSize) this.reverse = true;
      else if (this.size <= this.minSize) this.reverse = false;
      this.size += this.reverse ? -this.speed * 0.45 : this.speed * 0.45;
    } else {
      this.size = Math.min(this.maxSize, this.size + this.speed);
    }
  }

  disappear() {
    this.shimmering = false;
    this.counter = 0;
    if (this.size <= 0.05) {
      this.size = 0;
      this.idle = true;
      return;
    }
    this.size = Math.max(0, this.size - 0.24);
  }
}

function initShimmer() {
  const canvases = Array.from(document.querySelectorAll("canvas[data-shimmer]"));
  if (canvases.length === 0) return;

  const media = reducedMotionQuery();
  let reduce = media.matches;
  let frame = 0;
  const items = [];

  const schedule = () => {
    if (frame || reduce || document.hidden) return;
    frame = window.requestAnimationFrame(tick);
  };

  function buildPixels(item) {
    const rect = item.host.getBoundingClientRect();
    const width = Math.max(1, Math.round(rect.width));
    const height = Math.max(1, Math.round(rect.height));
    const dpr = Math.min(window.devicePixelRatio || 1, MAX_DPR);

    item.canvas.width = Math.round(width * dpr);
    item.canvas.height = Math.round(height * dpr);
    item.canvas.style.width = `${width}px`;
    item.canvas.style.height = `${height}px`;
    item.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    item.width = width;
    item.height = height;

    // Hover-only pixel shimmer: keep the canvas completely clean when not hovered.
    if (item.ambient) {
      item.pixels = [];
      item.ctx.clearRect(0, 0, width, height);
      return;
    }

    const gap = 9;
    const pixels = [];
    let n = item.seed;
    const nextRand = () => {
      n = (Math.imul(n, 1664525) + 1013904223) >>> 0;
      return n / 4294967296;
    };

    for (let x = gap * 0.5; x < width; x += gap) {
      for (let y = gap * 0.5; y < height; y += gap) {
        const color = PALETTE_CARD[Math.floor(nextRand() * PALETTE_CARD.length)];
        const speed = 0.09 + nextRand() * 0.18;
        const maxSize = 1.6 + nextRand() * 1.8;
        pixels.push(new ShimmerPixel(Math.round(x), Math.round(y), color, speed, maxSize));
      }
    }
    item.pixels = pixels;
    item.ctx.clearRect(0, 0, width, height);
  }

  function setRippleOrigin(item, ox, oy) {
    for (const px of item.pixels) {
      const dx = px.x - ox;
      const dy = px.y - oy;
      px.delay = Math.sqrt(dx * dx + dy * dy) * 0.55;
      px.counter = 0;
    }
  }

  function tick() {
    frame = 0;
    let keepGoing = false;

    for (const item of items) {
      if (!item.visible || item.ambient) continue;
      if (!item.active && !item.fading) continue;

      const { ctx, width, height, pixels } = item;
      ctx.clearRect(0, 0, width, height);
      let anyAlive = false;
      ctx.globalAlpha = 0.36;
      for (const px of pixels) {
        if (item.active) {
          px.appear(12);
          anyAlive = true;
        } else {
          px.disappear();
          if (!px.idle) anyAlive = true;
        }
        if (px.size > 0.05) {
          const off = (px.maxSize - px.size) * 0.5;
          ctx.fillStyle = px.color;
          ctx.fillRect(px.x + off, px.y + off, px.size, px.size);
        }
      }
      ctx.globalAlpha = 1;
      if (anyAlive) {
        keepGoing = true;
      } else {
        item.fading = false;
      }
    }

    if (keepGoing && !reduce && !document.hidden) {
      frame = window.requestAnimationFrame(tick);
    }
  }

  for (const [index, canvas] of canvases.entries()) {
    const host = canvas.parentElement;
    if (!host) continue;
    const ctx = canvas.getContext("2d", { alpha: true });
    if (!ctx) continue;

    const item = {
      canvas,
      host,
      ctx,
      ambient: canvas.hasAttribute("data-shimmer-ambient"),
      seed: 1337 + index * 997,
      pixels: [],
      width: 1,
      height: 1,
      active: false,
      fading: false,
      visible: true,
    };
    items.push(item);

    if ("ResizeObserver" in window) {
      new ResizeObserver(() => buildPixels(item)).observe(host);
    } else {
      window.addEventListener("resize", () => buildPixels(item));
    }

    if ("IntersectionObserver" in window) {
      new IntersectionObserver(
        (entries) => {
          for (const entry of entries) {
            item.visible = entry.isIntersecting;
            if (item.visible) schedule();
          }
        },
        { rootMargin: "100px" },
      ).observe(host);
    }

    if (!item.ambient) {
      host.addEventListener("pointerenter", (event) => {
        if (reduce) return;
        const rect = host.getBoundingClientRect();
        setRippleOrigin(item, event.clientX - rect.left, event.clientY - rect.top);
        item.active = true;
        item.fading = true;
        schedule();
      });
      host.addEventListener("pointerleave", () => {
        item.active = false;
        item.fading = true;
        schedule();
      });
    }

    buildPixels(item);
  }

  document.addEventListener("visibilitychange", () => {
    if (document.hidden && frame) {
      window.cancelAnimationFrame(frame);
      frame = 0;
    } else if (!document.hidden) {
      schedule();
    }
  });

  const onMotionChange = () => {
    reduce = media.matches;
    if (reduce && frame) {
      window.cancelAnimationFrame(frame);
      frame = 0;
      for (const item of items) {
        item.active = false;
        item.fading = false;
        item.ctx.clearRect(0, 0, item.width, item.height);
      }
    }
  };
  if (typeof media.addEventListener === "function") {
    media.addEventListener("change", onMotionChange);
  }
}

/**
 * Code-generated pixel frog hopping across the hero rolling road.
 * Each hop cycle (~1.35s) does: calm pause -> crouch -> parabolic arc leap -> land.
 */
function initHeroFrogHopper() {
  const wrap = document.getElementById("hero-road");
  const path = document.getElementById("hero-road-path");
  const walker = document.getElementById("hero-road-walker");
  const canvas = document.getElementById("hero-road-canvas");
  const shadow = wrap ? wrap.querySelector(".hero-road__shadow") : null;
  if (!wrap || !path || !walker || !canvas) return;

  const scale = 2;
  canvas.width = FROG_LOGICAL_W * scale;
  canvas.height = FROG_LOGICAL_H * scale;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  const media = reducedMotionQuery();
  let raf = 0;
  let total = 0;
  let start = 0;
  let lastKey = "";

  const HOPS_ACROSS = 9;
  const HOP_CYCLE_MS = 1450; // pause + crouch + jump + land
  const TOTAL_MS = HOPS_ACROSS * HOP_CYCLE_MS;

  function place(now) {
    const rect = wrap.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return;
    if (total === 0) total = path.getTotalLength();
    const ctm = path.getScreenCTM();
    if (!ctm || total === 0) return;

    let pathProgress = 0.68;
    let jumpArcPx = 0;
    let phase = "sit";
    let shadowScale = 1;

    if (!media.matches) {
      // Start slightly inside the left edge so the hopping frog is immediately visible on load.
      const elapsed = (now - start + HOP_CYCLE_MS * 1.45) % TOTAL_MS;
      const hopIdx = Math.floor(elapsed / HOP_CYCLE_MS);
      const t = (elapsed % HOP_CYCLE_MS) / HOP_CYCLE_MS; // 0..1 within one hop

      // 0.00..0.38: sit & pause | 0.38..0.50: crouch | 0.50..0.88: airborne leap | 0.88..1.00: land settle
      let hopAdvance = 0;
      if (t < 0.38) {
        phase = "sit";
        hopAdvance = 0;
      } else if (t < 0.5) {
        phase = "crouch";
        hopAdvance = 0;
        shadowScale = 1.12;
      } else if (t < 0.88) {
        const air = (t - 0.5) / 0.38; // 0..1
        hopAdvance = air;
        jumpArcPx = Math.sin(air * Math.PI) * 34;
        shadowScale = 1 - Math.sin(air * Math.PI) * 0.45;
        phase = air < 0.32 ? "up" : air < 0.68 ? "air" : "down";
      } else {
        phase = "crouch";
        hopAdvance = 1;
        shadowScale = 1.1;
      }

      pathProgress = clamp((hopIdx + hopAdvance) / HOPS_ACROSS, 0, 1);
    }

    const dist = total * pathProgress;
    const pt = path.getPointAtLength(dist).matrixTransform(ctm);
    const ahead = path.getPointAtLength(Math.min(total, dist + 6)).matrixTransform(ctm);
    const slopeDeg = phase === "up" || phase === "air" || phase === "down"
      ? 0
      : clamp((Math.atan2(ahead.y - pt.y, ahead.x - pt.x) * 180) / Math.PI, -7, 7);

    const blink = phase === "sit" && !media.matches && isBlinking(now);
    const key = `${phase}:${blink}`;
    if (key !== lastKey) {
      lastKey = key;
      renderFrogHopFrame(ctx, phase, { blink, scale, facingLeft: false });
    }

    const w = FROG_LOGICAL_W * scale;
    const h = FROG_LOGICAL_H * scale;
    const x = pt.x - rect.left - w * 0.5;
    const groundY = pt.y - rect.top - h + 4;
    walker.style.transform = `translate3d(${x.toFixed(1)}px, ${(groundY - jumpArcPx).toFixed(1)}px, 0) rotate(${slopeDeg.toFixed(1)}deg)`;
    if (shadow) {
      shadow.style.transform = `translate3d(0, ${jumpArcPx.toFixed(1)}px, 0) scaleX(${shadowScale.toFixed(2)})`;
      shadow.style.opacity = String(clamp(0.24 - jumpArcPx * 0.004, 0.06, 0.24));
    }
  }

  function loop(now) {
    raf = 0;
    if (document.hidden || media.matches) return;
    if (!start) start = now;
    place(now);
    raf = window.requestAnimationFrame(loop);
  }

  if (media.matches) {
    place(0);
  } else {
    raf = window.requestAnimationFrame(loop);
  }

  window.addEventListener("resize", () => {
    total = 0;
    if (media.matches) place(0);
  });
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden && !media.matches && !raf) {
      raf = window.requestAnimationFrame(loop);
    }
  });
}

export function initCraft() {
  initShimmer();
  initHeroFrogHopper();
}
