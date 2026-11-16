import type { PointerEvent as ReactPointerEvent } from "react";

/** Moving the mouse further than this (px) while pressed turns a click into a drag. */
const DRAG_SLOP = 6;
/** Pointer samples older than this (ms) do not count towards the release speed. */
const SPEED_WINDOW_MS = 90;
/** Share of the speed left after each millisecond of gliding (about 0.95 per 60 Hz frame). */
const FRICTION_PER_MS = 0.997;
/** The glide ends below this speed (px/ms). */
const MIN_SPEED = 0.02;

/** The glide of each row, so a new press on it stops the one still running. */
const glides = new WeakMap<HTMLElement, number>();

function stopGlide(el: HTMLElement): void {
  const id = glides.get(el);
  if (id !== undefined) cancelAnimationFrame(id);
  glides.delete(el);
}

/** Let the row run on from `speed` (px/ms, positive scrolls right) and slow down by itself. */
function glide(el: HTMLElement, speed: number): void {
  let v = speed;
  let last = performance.now();
  // Fractions of a pixel are carried over: `scrollLeft` rounds on some scale factors.
  let x = el.scrollLeft;
  const step = (now: number) => {
    // Something else moved the row (wheel, arrow button, keyboard): it has the say.
    if (Math.abs(el.scrollLeft - x) > 2) {
      glides.delete(el);
      return;
    }
    const dt = Math.min(48, now - last);
    last = now;
    v *= Math.pow(FRICTION_PER_MS, dt);
    x += v * dt;
    const max = el.scrollWidth - el.clientWidth;
    if (Math.abs(v) < MIN_SPEED || x <= 0 || x >= max) {
      el.scrollLeft = Math.min(max, Math.max(0, x));
      glides.delete(el);
      return;
    }
    el.scrollLeft = x;
    glides.set(el, requestAnimationFrame(step));
  };
  glides.set(el, requestAnimationFrame(step));
}

/**
 * `onPointerDown` handler for click-and-drag horizontal scrolling with the mouse, gliding on after the release.
 * A drag swallows the click that ends it, so letting go over a card does not open it.
 *
 * The row is moved once per frame however often the mouse reports (gaming mice report 1000 times a second, and a
 * scroll write per report makes the page lay out that often), and the cards ignore the pointer while it drags, so
 * their hover effects do not repaint as they pass under it.
 */
export function dragScroll(e: ReactPointerEvent<HTMLElement>): void {
  const el = e.currentTarget;
  if (e.pointerType !== "mouse" || e.button !== 0) return;
  stopGlide(el);
  const startX = e.clientX;
  const startLeft = el.scrollLeft;
  let dragging = false;
  let latestX = startX;
  let frame: number | undefined;
  const samples: { x: number; t: number }[] = [{ x: startX, t: performance.now() }];
  const apply = () => {
    frame = undefined;
    el.scrollLeft = startLeft - (latestX - startX);
  };
  const onMove = (m: PointerEvent) => {
    if (!dragging && Math.abs(m.clientX - startX) < DRAG_SLOP) return;
    if (!dragging) {
      dragging = true;
      el.classList.add("dragging");
    }
    latestX = m.clientX;
    const now = performance.now();
    samples.push({ x: latestX, t: now });
    while (samples.length > 2 && now - (samples[0]?.t ?? now) > SPEED_WINDOW_MS * 2) samples.shift();
    frame ??= requestAnimationFrame(apply);
  };
  const onUp = () => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    window.removeEventListener("pointercancel", onUp);
    if (!dragging) return;
    if (frame !== undefined) {
      cancelAnimationFrame(frame);
      apply();
    }
    el.classList.remove("dragging");
    const swallow = (c: MouseEvent) => {
      c.stopPropagation();
      c.preventDefault();
    };
    window.addEventListener("click", swallow, { capture: true, once: true });
    window.setTimeout(() => {
      window.removeEventListener("click", swallow, { capture: true });
    }, 0);
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    // Speed over the last moments before the release; a pointer that stood still before letting go does not fling.
    const now = performance.now();
    const recent = samples.filter((s) => now - s.t <= SPEED_WINDOW_MS);
    const first = recent[0];
    const last = recent[recent.length - 1];
    if (!first || !last || last.t - first.t < 8) return;
    const speed = -(last.x - first.x) / (last.t - first.t);
    if (Math.abs(speed) > 0.15) glide(el, speed);
  };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
  window.addEventListener("pointercancel", onUp);
}
