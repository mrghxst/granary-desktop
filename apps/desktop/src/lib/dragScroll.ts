import type { PointerEvent as ReactPointerEvent } from "react";

/** Moving the mouse further than this (px) while pressed turns a click into a drag. */
const DRAG_SLOP = 6;

/**
 * `onPointerDown` handler for click-and-drag horizontal scrolling with the mouse, with a little momentum on release.
 * A drag swallows the click that ends it, so letting go over a card does not open it.
 */
export function dragScroll(e: ReactPointerEvent<HTMLElement>): void {
  const el = e.currentTarget;
  if (e.pointerType !== "mouse" || e.button !== 0) return;
  const startX = e.clientX;
  const startLeft = el.scrollLeft;
  let dragging = false;
  let lastX = startX;
  let lastT = performance.now();
  let velocity = 0;
  const onMove = (m: PointerEvent) => {
    const dx = m.clientX - startX;
    if (!dragging && Math.abs(dx) < DRAG_SLOP) return;
    if (!dragging) {
      dragging = true;
      el.classList.add("dragging");
    }
    const now = performance.now();
    velocity = (m.clientX - lastX) / Math.max(1, now - lastT);
    lastX = m.clientX;
    lastT = now;
    el.scrollLeft = startLeft - dx;
  };
  const onUp = () => {
    window.removeEventListener("pointermove", onMove);
    window.removeEventListener("pointerup", onUp);
    window.removeEventListener("pointercancel", onUp);
    if (!dragging) return;
    el.classList.remove("dragging");
    const swallow = (c: MouseEvent) => {
      c.stopPropagation();
      c.preventDefault();
    };
    window.addEventListener("click", swallow, { capture: true, once: true });
    window.setTimeout(() => {
      window.removeEventListener("click", swallow, { capture: true });
    }, 0);
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    const fling = -velocity * 220;
    if (!reduce && Math.abs(fling) > 40) el.scrollBy({ left: fling, behavior: "smooth" });
  };
  window.addEventListener("pointermove", onMove);
  window.addEventListener("pointerup", onUp);
  window.addEventListener("pointercancel", onUp);
}
