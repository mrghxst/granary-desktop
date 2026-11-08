"use client";
import { useEffect, useRef, type CSSProperties, type ReactNode } from "react";

/** Fades and lifts its content in the first time it scrolls into view. */
export function Reveal({
  children,
  delay = 0,
  className = "",
  as: Tag = "div",
}: {
  children: ReactNode;
  delay?: number;
  className?: string;
  as?: "div" | "section" | "li";
}) {
  const ref = useRef<HTMLElement>(null);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (typeof IntersectionObserver === "undefined") {
      el.dataset.shown = "true";
      return;
    }
    const io = new IntersectionObserver(
      ([entry]) => {
        if (entry?.isIntersecting) {
          el.dataset.shown = "true";
          io.disconnect();
        }
      },
      { rootMargin: "0px 0px -12% 0px" },
    );
    io.observe(el);
    return () => io.disconnect();
  }, []);
  const style = { "--delay": `${delay}ms` } as CSSProperties;
  return (
    // @ts-expect-error -- polymorphic ref: all three tags are HTMLElements
    <Tag ref={ref} data-reveal="" style={style} className={className}>
      {children}
    </Tag>
  );
}
