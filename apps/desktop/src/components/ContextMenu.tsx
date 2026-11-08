import { useEffect, useRef, type ReactNode } from "react";
import { createPortal } from "react-dom";

export interface MenuAction {
  label: string;
  onSelect: () => void;
  icon?: ReactNode;
}

/**
 * A small right-click menu positioned at the pointer; Escape, outside click, scrolling or blur closes it. Rendered into
 * `document.body`: inside a card, the card's hover transform and the row's overflow would otherwise make the fixed
 * menu position against the card and clip it.
 */
export function ContextMenu({
  x,
  y,
  actions,
  onClose,
  label,
}: {
  x: number;
  y: number;
  actions: MenuAction[];
  onClose: () => void;
  label: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  // Parents pass an inline onClose; read it through a ref so a parent re-render neither re-subscribes the listeners
  // nor moves focus back to the first item (phase-5 review L11).
  const closeRef = useRef(onClose);
  useEffect(() => {
    closeRef.current = onClose;
  });

  // Focus the first item once, when the menu opens.
  useEffect(() => {
    ref.current?.querySelector<HTMLButtonElement>("button")?.focus();
  }, []);

  useEffect(() => {
    const close = () => {
      closeRef.current();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") close();
    };
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) close();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("mousedown", onDown);
    window.addEventListener("blur", close);
    window.addEventListener("wheel", close, { passive: true });
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("wheel", close);
      window.removeEventListener("resize", close);
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("blur", close);
    };
  }, []);

  // Keep the menu inside the viewport.
  const left = Math.min(x, window.innerWidth - 220);
  const top = Math.min(y, window.innerHeight - (actions.length * 40 + 16));

  return createPortal(
    <div
      ref={ref}
      role="menu"
      tabIndex={-1}
      aria-label={label}
      onContextMenu={(e) => {
        e.preventDefault();
      }}
      className="fixed z-[70] min-w-48 rounded-lg border border-border bg-popover p-1 shadow-2xl"
      style={{ left, top }}
      onKeyDown={(e) => {
        if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
        e.preventDefault();
        const items = [...(ref.current?.querySelectorAll<HTMLButtonElement>("button") ?? [])];
        const i = items.indexOf(document.activeElement as HTMLButtonElement);
        const next = e.key === "ArrowDown" ? (i + 1) % items.length : (i - 1 + items.length) % items.length;
        items[next]?.focus();
      }}
    >
      {actions.map((a) => (
        <button
          key={a.label}
          type="button"
          role="menuitem"
          className="flex w-full items-center gap-2 rounded-md px-3 py-2 text-left text-sm hover:bg-surface-hover focus:bg-surface-hover focus:outline-none"
          onClick={() => {
            onClose();
            a.onSelect();
          }}
        >
          {a.icon}
          {a.label}
        </button>
      ))}
    </div>,
    document.body,
  );
}
