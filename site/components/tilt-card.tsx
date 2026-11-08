"use client";
import { useEffect, useRef, type ReactNode } from "react";

const MAX_DEG = 9;

/**
 * A card that tilts toward the pointer in 3D, with a soft glare that follows it and a spring back to flat on leave.
 * Pointer only (no gyroscope); off with prefers-reduced-motion.
 */
export function TiltCard({
  children,
  className = "",
}: {
  children: ReactNode;
  className?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const reduced = window.matchMedia("(prefers-reduced-motion: reduce)");
    let frame = 0;
    // Current and target state: rotation in degrees, glare position in %, glare strength.
    const cur = { rx: 0, ry: 0, gx: 50, gy: 0, g: 0 };
    const goal = { ...cur };
    const apply = () => {
      el.style.setProperty("--rx", `${cur.rx.toFixed(2)}deg`);
      el.style.setProperty("--ry", `${cur.ry.toFixed(2)}deg`);
      el.style.setProperty("--gx", `${cur.gx.toFixed(1)}%`);
      el.style.setProperty("--gy", `${cur.gy.toFixed(1)}%`);
      el.style.setProperty("--g", cur.g.toFixed(3));
    };
    const step = () => {
      let moving = false;
      for (const k of Object.keys(cur) as (keyof typeof cur)[]) {
        const d = goal[k] - cur[k];
        cur[k] += d * 0.12;
        if (Math.abs(d) > 0.01) moving = true;
      }
      apply();
      frame = moving ? requestAnimationFrame(step) : 0;
    };
    const kick = () => {
      if (!frame) frame = requestAnimationFrame(step);
    };
    const move = (e: PointerEvent) => {
      if (reduced.matches || e.pointerType === "touch") return;
      const r = el.getBoundingClientRect();
      const x = (e.clientX - r.left) / r.width;
      const y = (e.clientY - r.top) / r.height;
      goal.ry = (x - 0.5) * 2 * MAX_DEG;
      goal.rx = -(y - 0.5) * 2 * MAX_DEG * 0.7;
      goal.gx = x * 100;
      goal.gy = y * 100;
      goal.g = 1;
      kick();
    };
    const leave = () => {
      Object.assign(goal, { rx: 0, ry: 0, g: 0 });
      kick();
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerleave", leave);
    return () => {
      cancelAnimationFrame(frame);
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerleave", leave);
    };
  }, []);

  return (
    <div ref={ref} className={`tilt-3d ${className}`}>
      <div className="tilt-3d-inner">
        {children}
        <div aria-hidden className="tilt-3d-glare" />
      </div>
    </div>
  );
}
