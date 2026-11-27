/**
 * Handmade interactions for the StackSpirit marketing site.
 *
 * Two progressive enhancements, both dependency-free:
 *
 *   1. A pixel-shimmer canvas behind the hero and each feature card. The hero
 *      gets a slow ambient field; cards get a still field plus a little sparkle
 *      under the pointer. Everything is drawn as flat Meadow-colour squares so
 *      it stays crisp and cheap.
 *   2. A scroll-driven curved SVG path with the thick pixel mascot walking
 *      down the page. The mascot position comes from the path's own geometry,
 *      so the curve, the page height and resizes all stay in sync.
 *
 * `prefers-reduced-motion: reduce` disables both animations: the canvas paints
 * one still field and the mascot sits at a fixed point on the path.
 */

const MEADOW = ["67,168,90", "205,237,176", "30,91,54"];
const MAX_DPR = 2;

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

function reducedMotionQuery() {
  return window.matchMedia("(prefers-reduced-motion: reduce)");
}

function hash(seed) {
  let value = seed | 0;
  value = Math.imul(value ^ (value >>> 16), 0x45d9f3b);
  value = Math.imul(value ^ (value >>> 16), 0x45d9f3b);
  return (value ^ (value >>> 16)) >>> 0;
}

function fieldFor(cols, rows, seed) {
  const dots = [];
  for (let row = 0; row < rows; row += 1) {
    for (let col = 0; col < cols; col += 1) {
      const noise = hash(seed + col * 374761 + row * 668265);
      if (noise % 100 > 26) continue;
      dots.push({
        col,
        row,
        tone: noise % MEADOW.length,
        alpha: 0.1 + ((noise >>> 8) % 22) / 100,
        phase: ((noise >>> 12) % 628) / 100,
        speed: 0.4 + ((noise >>> 18) % 70) / 100,
        size: 2 + ((noise >>> 22) % 2) * 2,
      });
    }
  }
  return dots;
}

function drawPixel(ctx, x, y, size, tone, alpha) {
  if (alpha <= 0.005) return;
  ctx.fillStyle = `rgba(${MEADOW[tone]},${alpha.toFixed(3)})`;
  ctx.fillRect(x, y, size, size);
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

  function tick(time) {
    frame = 0;
    let active = false;

    for (const item of items) {
      if (!item.visible) continue;

      let expired = false;
      for (const [key, spark] of item.sparks) {
        if (spark.until <= time) {
          item.sparks.delete(key);
          expired = true;
        }
      }

      const busy = item.ambient || item.hovering || item.sparks.size > 0;
      if (busy) {
        active = true;
        paint(item, time, true);
      } else if (expired) {
        // Leave the still field behind after the last spark fades, not the
        // near-invisible frame where every spark had run out.
        paint(item, 0, false);
      }
    }

    if (active && !reduce && !document.hidden) {
      frame = window.requestAnimationFrame(tick);
    }
  }

  function paint(item, time, animated) {
    const { ctx, canvas, dots, cell } = item;
    const width = canvas.width / item.dpr;
    const height = canvas.height / item.dpr;
    ctx.clearRect(0, 0, width, height);

    for (const dot of dots) {
      const x = dot.col * cell + 1;
      const y = dot.row * cell + 1;
      let alpha = dot.alpha;
      if (animated && item.ambient) {
        alpha *= 0.55 + 0.45 * Math.sin(time * 0.001 * dot.speed + dot.phase);
      }
      if (animated && item.hovering) alpha *= 1.45;
      drawPixel(ctx, x, y, dot.size, dot.tone, alpha);
    }

    if (item.sparks.size > 0) {
      for (const spark of item.sparks.values()) {
        const remaining = clamp((spark.until - time) / 520, 0, 1);
        const x = spark.col * cell + 1;
        const y = spark.row * cell + 1;
        drawPixel(ctx, x, y, cell - 3, spark.tone, 0.18 + remaining * 0.55);
        drawPixel(ctx, x + 2, y + 2, Math.max(2, cell - 8), spark.tone, 0.1 + remaining * 0.5);
      }
    }
  }

  function size(item) {
    const rect = item.host.getBoundingClientRect();
    const width = Math.max(1, Math.round(rect.width));
    const height = Math.max(1, Math.round(rect.height));
    const dpr = Math.min(window.devicePixelRatio || 1, MAX_DPR);
    item.canvas.width = Math.round(width * dpr);
    item.canvas.height = Math.round(height * dpr);
    item.canvas.style.width = `${width}px`;
    item.canvas.style.height = `${height}px`;
    item.ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    item.dpr = dpr;
    item.cols = Math.ceil(width / item.cell);
    item.rows = Math.ceil(height / item.cell);
    item.dots = fieldFor(item.cols, item.rows, item.seed);
    paint(item, 0, false);
  }

  for (const [index, canvas] of canvases.entries()) {
    const host = canvas.parentElement;
    if (!host) continue;

    const item = {
      canvas,
      host,
      ctx: canvas.getContext("2d", { alpha: true }),
      ambient: canvas.hasAttribute("data-shimmer-ambient"),
      cell: host.classList.contains("hero") ? 16 : 12,
      seed: 97 + index * 131,
      dots: [],
      sparks: new Map(),
      cols: 0,
      rows: 0,
      dpr: Math.min(window.devicePixelRatio || 1, MAX_DPR),
      hovering: false,
      visible: true,
      cleanups: [],
    };
    if (!item.ctx) continue;
    items.push(item);

    if ("ResizeObserver" in window) {
      const observer = new ResizeObserver(() => size(item));
      observer.observe(host);
      item.cleanups.push(() => observer.disconnect());
    } else {
      const onResize = () => size(item);
      window.addEventListener("resize", onResize);
      item.cleanups.push(() => window.removeEventListener("resize", onResize));
    }

    if ("IntersectionObserver" in window) {
      const observer = new IntersectionObserver(
        (entries) => {
          for (const entry of entries) {
            item.visible = entry.isIntersecting;
            if (item.visible) {
              schedule();
            } else {
              item.hovering = false;
              item.sparks.clear();
              paint(item, 0, false);
            }
          }
        },
        { rootMargin: "80px" },
      );
      observer.observe(host);
      item.cleanups.push(() => observer.disconnect());
    }

    host.addEventListener("pointermove", (event) => {
      if (reduce) return;
      const rect = host.getBoundingClientRect();
      const col = Math.floor((event.clientX - rect.left) / item.cell);
      const row = Math.floor((event.clientY - rect.top) / item.cell);
      if (col < 0 || row < 0 || col >= item.cols || row >= item.rows) return;
      const until = performance.now() + 900;
      for (let dy = -1; dy <= 1; dy += 1) {
        for (let dx = -1; dx <= 1; dx += 1) {
          const key = `${col + dx},${row + dy}`;
          if (item.sparks.size > 90 && !item.sparks.has(key)) continue;
          item.sparks.set(key, {
            col: col + dx,
            row: row + dy,
            tone: (col + row + dx + dy + item.seed) % MEADOW.length,
            until: until - Math.abs(dx) * 80 - Math.abs(dy) * 80,
          });
        }
      }
      item.hovering = true;
      schedule();
    });

    host.addEventListener("pointerleave", () => {
      item.hovering = false;
      schedule();
    });

    size(item);
  }

  const onVisibility = () => {
    if (document.hidden && frame) {
      window.cancelAnimationFrame(frame);
      frame = 0;
    } else if (!document.hidden) {
      schedule();
    }
  };
  document.addEventListener("visibilitychange", onVisibility);

  const onMotionChange = () => {
    reduce = media.matches;
    if (reduce) {
      if (frame) window.cancelAnimationFrame(frame);
      frame = 0;
      for (const item of items) {
        item.sparks.clear();
        item.hovering = false;
        paint(item, 0, false);
      }
    } else {
      schedule();
    }
  };
  if (typeof media.addEventListener === "function") {
    media.addEventListener("change", onMotionChange);
  } else if (typeof media.addListener === "function") {
    media.addListener(onMotionChange);
  }

  schedule();
}

function initScrollPath() {
  const wrap = document.getElementById("scroll-path");
  const line = document.getElementById("scroll-path-line");
  const progress = document.getElementById("scroll-path-progress");
  const mascot = document.getElementById("scroll-path-mascot");
  const sprite = mascot ? mascot.querySelector(".scroll-path__sprite") : null;
  if (!wrap || !line || !progress || !mascot || !sprite) return;

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

    const anchor = window.innerHeight * 0.55;
    const ratio = media.matches
      ? 0.22
      : clamp((anchor - rect.top) / rect.height, 0, 1);

    progress.style.strokeDasharray = `${length} ${length}`;
    progress.style.strokeDashoffset = `${length * (1 - ratio)}`;

    const matrix = line.getScreenCTM();
    if (!matrix) return;
    const point = line.getPointAtLength(length * ratio);
    const screen = point.matrixTransform(matrix);
    const width = mascot.offsetWidth || 56;
    const height = mascot.offsetHeight || 89;
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
  } else if (typeof media.addListener === "function") {
    media.addListener(requestUpdate);
  }

  requestUpdate();
}

export function initCraft() {
  initShimmer();
  initScrollPath();
}
