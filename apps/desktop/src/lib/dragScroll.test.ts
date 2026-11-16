import type { PointerEvent as ReactPointerEvent } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { dragScroll } from "./dragScroll";

describe("dragScroll", () => {
  let frames: FrameRequestCallback[] = [];
  const row = () => {
    const el = document.createElement("div");
    document.body.append(el);
    return el;
  };
  const press = (el: HTMLElement, clientX: number) => {
    dragScroll({
      currentTarget: el,
      pointerType: "mouse",
      button: 0,
      clientX,
    } as unknown as ReactPointerEvent<HTMLElement>);
  };
  const move = (clientX: number) => window.dispatchEvent(new MouseEvent("pointermove", { clientX }));
  const release = () => window.dispatchEvent(new MouseEvent("pointerup"));

  beforeEach(() => {
    frames = [];
    vi.stubGlobal("requestAnimationFrame", (f: FrameRequestCallback) => frames.push(f));
    vi.stubGlobal("cancelAnimationFrame", () => {
      frames = [];
    });
    vi.stubGlobal("matchMedia", () => ({ matches: true }));
  });
  afterEach(async () => {
    // The listener that swallows the click after a drag goes with the next task.
    await new Promise((r) => setTimeout(r, 0));
    vi.unstubAllGlobals();
    document.body.replaceChildren();
  });

  it("moves the row with the mouse, in the move itself", () => {
    const el = row();
    el.scrollLeft = 100;
    press(el, 500);
    move(497);
    expect(el.scrollLeft).toBe(100);
    move(440);
    expect(el.scrollLeft).toBe(160);
    expect(frames).toHaveLength(0);
    expect(el.classList.contains("dragging")).toBe(true);
    release();
    expect(el.classList.contains("dragging")).toBe(false);
  });

  it("leaves a click alone and swallows the one that ends a drag", () => {
    const el = row();
    const clicked = vi.fn();
    el.addEventListener("click", clicked);
    press(el, 500);
    move(498);
    release();
    el.click();
    expect(clicked).toHaveBeenCalledTimes(1);
    press(el, 500);
    move(400);
    release();
    el.click();
    expect(clicked).toHaveBeenCalledTimes(1);
  });

  it("ignores touch and other buttons", () => {
    const el = row();
    dragScroll({
      currentTarget: el,
      pointerType: "touch",
      button: 0,
      clientX: 500,
    } as unknown as ReactPointerEvent<HTMLElement>);
    move(300);
    expect(frames).toHaveLength(0);
    expect(el.classList.contains("dragging")).toBe(false);
  });
});
