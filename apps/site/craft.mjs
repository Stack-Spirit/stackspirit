/**
 * Handmade interactions for the StackSpirit marketing site.
 *
 * Four dependency-free progressive enhancements:
 *   1. Authentic pixel-canvas shimmer (ExampleDesign/pixel-canvas): tight
 *      Meadow pixel cells that ripple outward from the cursor on card hover.
 *   2. Code-generated 8-frame pixel-film hero walker: the skinny deadpan
 *      frog-eyed sprout spirit walks across a North Noir rolling Meadow curve
 *      at the bottom of the hero, blinking and leaning into the hill slope.
 *   3. Code-generated blinking spirit icons in the header and developer note.
 *   4. Scroll-driven vertical Meadow path on the page edge with the same
 *      code-generated skinny pixel walker tracking scroll progress.
 *
 * `prefers-reduced-motion: reduce` disables animation cleanly.
 */

import {
  ICON_LOGICAL_H,
  ICON_LOGICAL_W,
  WALKER_LOGICAL_H,
  WALKER_LOGICAL_W,
  renderSproutIcon,
  renderWalkerFrame,
} from "./pixel-film.mjs";

const PALETTE_CARD = ["#cdedb0", "#43a85a", "#2e7f48", "#1e5b36"];
const PALETTE_HERO = ["#cdedb0", "#43a85a", "#1e5b36"];
const MAX_DPR = 2;

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

function reducedMotionQuery() {
  return window.matchMedia("(prefers-reduced-motion: reduce)");
}

function isBlinking(timeMs) {
  const cycle = timeMs % 3900;
  return cycle > 3720 && cycle < 3860;
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

    const gap = item.ambient ? 20 : 9;
    const palette = item.ambient ? PALETTE_HERO : PALETTE_CARD;
    const pixels = [];
    let n = item.seed;
    const nextRand = () => {
      n = (Math.imul(n, 1664525) + 1013904223) >>> 0;
      return n / 4294967296;
    };

    for (let x = gap * 0.5; x < width; x += gap) {
      for (let y = gap * 0.5; y < height; y += gap) {
        if (item.ambient) {
          const nx = x / width;
          const ny = y / height;
          const edgeBias = Math.max(nx - 0.45, ny - 0.55);
          if (edgeBias < 0 || nextRand() > edgeBias * 0.36) continue;
        }
        const color = palette[Math.floor(nextRand() * palette.length)];
        const speed = 0.08 + nextRand() * 0.18;
        const maxSize = item.ambient ? 2 + Math.floor(nextRand() * 2) : 1.5 + nextRand() * 1.8;
        const px = new ShimmerPixel(Math.round(x), Math.round(y), color, speed, maxSize);
        if (item.ambient) {
          px.phase = nextRand() * Math.PI * 2;
          px.size = maxSize * 0.65;
        }
        pixels.push(px);
      }
    }
    item.pixels = pixels;
    drawStatic(item);
  }

  function setRippleOrigin(item, ox, oy) {
    for (const px of item.pixels) {
      const dx = px.x - ox;
      const dy = px.y - oy;
      px.delay = Math.sqrt(dx * dx + dy * dy) * 0.55;
      px.counter = 0;
    }
  }

  function drawStatic(item) {
    const { ctx, width, height, pixels, ambient } = item;
    ctx.clearRect(0, 0, width, height);
    if (!ambient) return;
    ctx.globalAlpha = 0.2;
    for (const px of pixels) {
      ctx.fillStyle = px.color;
      ctx.fillRect(px.x, px.y, px.maxSize, px.maxSize);
    }
    ctx.globalAlpha = 1;
  }

  function tick(time) {
    frame = 0;
    let keepGoing = false;

    for (const item of items) {
      if (!item.visible) continue;
      const { ctx, width, height, pixels, ambient } = item;

      if (ambient) {
        keepGoing = true;
        ctx.clearRect(0, 0, width, height);
        for (const px of pixels) {
          const wave = 0.5 + 0.5 * Math.sin(time * 0.0011 + (px.phase || 0));
          ctx.globalAlpha = 0.06 + wave * 0.22;
          ctx.fillStyle = px.color;
          ctx.fillRect(px.x, px.y, px.maxSize, px.maxSize);
        }
        ctx.globalAlpha = 1;
        continue;
      }

      if (!item.active && !item.fading) continue;

      ctx.clearRect(0, 0, width, height);
      let anyAlive = false;
      ctx.globalAlpha = 0.34;
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
        drawStatic(item);
      }
    } else if (!reduce) {
      schedule();
    }
  };
  if (typeof media.addEventListener === "function") {
    media.addEventListener("change", onMotionChange);
  }

  schedule();
}

/** Code-generated pixel icons (header mark & developer field-note idle buddy). */
function initCodeSprites() {
  const icons = Array.from(document.querySelectorAll("canvas[data-pixel-icon]"));
  const idles = Array.from(document.querySelectorAll("canvas[data-pixel-idle]"));
  if (icons.length === 0 && idles.length === 0) return;

  const media = reducedMotionQuery();

  for (const canvas of icons) {
    const scale = Number(canvas.getAttribute("data-scale") || 2);
    canvas.width = ICON_LOGICAL_W * scale;
    canvas.height = ICON_LOGICAL_H * scale;
  }
  for (const canvas of idles) {
    const scale = Number(canvas.getAttribute("data-scale") || 2);
    canvas.width = WALKER_LOGICAL_W * scale;
    canvas.height = WALKER_LOGICAL_H * scale;
  }

  function paintAll(now) {
    const blink = !media.matches && isBlinking(now);
    const sway = media.matches ? 0 : Math.floor(now / 950) % 3;
    for (const canvas of icons) {
      const ctx = canvas.getContext("2d");
      if (!ctx) continue;
      const scale = Number(canvas.getAttribute("data-scale") || 2);
      renderSproutIcon(ctx, { blink, sway, scale });
    }
    for (const canvas of idles) {
      const ctx = canvas.getContext("2d");
      if (!ctx) continue;
      const scale = Number(canvas.getAttribute("data-scale") || 2);
      renderWalkerFrame(ctx, -1, { blink, scale });
    }
  }

  paintAll(0);
  if (media.matches) return;

  let raf = 0;
  let lastPaint = 0;
  function tick(now) {
    raf = 0;
    if (document.hidden) return;
    if (now - lastPaint >= 120) {
      lastPaint = now;
      paintAll(now);
    }
    raf = window.requestAnimationFrame(tick);
  }
  raf = window.requestAnimationFrame(tick);
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden && !media.matches && !raf) {
      raf = window.requestAnimationFrame(tick);
    }
  });
}

/** North Noir-inspired hero road with our 8-frame code-generated skinny pixel walker. */
function initHeroWalker() {
  const wrap = document.getElementById("hero-road");
  const path = document.getElementById("hero-road-path");
  const walker = document.getElementById("hero-road-walker");
  const canvas = document.getElementById("hero-road-canvas");
  if (!wrap || !path || !walker || !canvas) return;

  const scale = 2;
  canvas.width = WALKER_LOGICAL_W * scale;
  canvas.height = WALKER_LOGICAL_H * scale;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  const media = reducedMotionQuery();
  let raf = 0;
  let total = 0;
  let start = 0;
  let lastFrameIdx = -99;
  let lastBlink = false;
  const DURATION_MS = 20000;
  const FRAME_MS = 125; // 8 fps crisp pixel-film cadence

  function place(progress, now) {
    const rect = wrap.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return;
    if (total === 0) total = path.getTotalLength();
    const ctm = path.getScreenCTM();
    if (!ctm || total === 0) return;

    const dist = total * clamp(progress, 0, 1);
    const pt = path.getPointAtLength(dist).matrixTransform(ctm);
    const ahead = path.getPointAtLength(Math.min(total, dist + 6)).matrixTransform(ctm);
    const slopeDeg = clamp((Math.atan2(ahead.y - pt.y, ahead.x - pt.x) * 180) / Math.PI, -8, 8);

    const frameIdx = media.matches ? -1 : Math.floor(now / FRAME_MS) % 8;
    const blink = !media.matches && isBlinking(now);
    if (frameIdx !== lastFrameIdx || blink !== lastBlink) {
      lastFrameIdx = frameIdx;
      lastBlink = blink;
      renderWalkerFrame(ctx, frameIdx, { blink, scale, facingLeft: false });
    }

    const w = walker.offsetWidth || WALKER_LOGICAL_W * scale;
    const h = walker.offsetHeight || WALKER_LOGICAL_H * scale;
    const x = pt.x - rect.left - w * 0.5;
    const y = pt.y - rect.top - h + 4;
    walker.style.transform = `translate3d(${x.toFixed(1)}px, ${y.toFixed(1)}px, 0) rotate(${slopeDeg.toFixed(1)}deg)`;
  }

  function loop(now) {
    raf = 0;
    if (document.hidden || media.matches) return;
    if (!start) start = now;
    const progress = ((now - start) % DURATION_MS) / DURATION_MS;
    place(progress, now);
    raf = window.requestAnimationFrame(loop);
  }

  if (media.matches) {
    place(0.62, 0);
  } else {
    raf = window.requestAnimationFrame(loop);
  }

  window.addEventListener("resize", () => {
    total = 0;
    if (media.matches) place(0.62, 0);
  });
  document.addEventListener("visibilitychange", () => {
    if (!document.hidden && !media.matches && !raf) {
      raf = window.requestAnimationFrame(loop);
    }
  });
}

/** Scroll-driven Meadow spine path with code-generated skinny pixel walker. */
function initScrollPath() {
  const wrap = document.getElementById("scroll-path");
  const line = document.getElementById("scroll-path-line");
  const progress = document.getElementById("scroll-path-progress");
  const mascot = document.getElementById("scroll-path-mascot");
  const canvas = mascot ? mascot.querySelector("canvas.scroll-path__sprite") : null;
  if (!wrap || !line || !progress || !mascot || !canvas) return;

  const scale = 2;
  canvas.width = WALKER_LOGICAL_W * scale;
  canvas.height = WALKER_LOGICAL_H * scale;
  const ctx = canvas.getContext("2d");
  if (!ctx) return;

  const media = reducedMotionQuery();
  let frame = 0;
  let length = 0;
  let flipped = false;

  function visible() {
    return window.getComputedStyle(wrap).display !== "none" && wrap.getBoundingClientRect().height > 0;
  }

  function update() {
    frame = 0;
    if (!visible()) return;

    if (length === 0) length = line.getTotalLength();
    const rect = wrap.getBoundingClientRect();
    if (rect.height === 0 || length === 0) return;

    const anchor = window.innerHeight * 0.5;
    const ratio = media.matches
      ? 0.18
      : clamp((anchor - rect.top) / rect.height, 0, 1);

    progress.style.strokeDasharray = `${length} ${length}`;
    progress.style.strokeDashoffset = `${length * (1 - ratio)}`;

    const matrix = line.getScreenCTM();
    if (!matrix) return;
    const point = line.getPointAtLength(length * ratio);
    const screen = point.matrixTransform(matrix);
    const width = mascot.offsetWidth || WALKER_LOGICAL_W * scale;
    const height = mascot.offsetHeight || WALKER_LOGICAL_H * scale;
    mascot.style.transform = `translate3d(${screen.x - rect.left - width / 2}px, ${
      screen.y - rect.top - height / 2
    }px, 0)`;

    if (!media.matches && ratio < 0.995) {
      const nextPoint = line.getPointAtLength(Math.min(length, length * ratio + 2));
      const nextScreen = nextPoint.matrixTransform(matrix);
      const shouldFlip = nextScreen.x < screen.x - 0.5;
      if (shouldFlip !== flipped) {
        flipped = shouldFlip;
        mascot.classList.toggle("is-flipped", shouldFlip);
      }
    }

    const walkStep = media.matches ? -1 : Math.floor(ratio * 48) % 8;
    renderWalkerFrame(ctx, walkStep, { blink: false, scale, facingLeft: false });
  }

  function requestUpdate() {
    if (frame) return;
    frame = window.requestAnimationFrame(update);
  }

  window.addEventListener("scroll", requestUpdate, { passive: true });
  window.addEventListener("resize", requestUpdate);
  window.addEventListener("load", requestUpdate, { once: true });

  if (typeof media.addEventListener === "function") {
    media.addEventListener("change", requestUpdate);
  }

  requestUpdate();
}

export function initCraft() {
  initShimmer();
  initCodeSprites();
  initHeroWalker();
  initScrollPath();
}
