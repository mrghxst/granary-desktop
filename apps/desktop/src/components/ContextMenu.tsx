import { useEffect, useRef, type ReactNode } from "react";

export interface MenuAction {
  label: string;
  onSelect: () => void;
  icon?: ReactNode;
}

/** A small right-click menu positioned at the pointer; Escape, outside click or blur closes it. */
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

  useEffect(() => {
    ref.current?.querySelector<HTMLButtonElement>("button")?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) onClose();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("mousedown", onDown);
    window.addEventListener("blur", onClose);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mousedown", onDown);
      window.removeEventListener("blur", onClose);
    };
  }, [onClose]);

  // Keep the menu inside the viewport.
  const left = Math.min(x, window.innerWidth - 220);
  const top = Math.min(y, window.innerHeight - (actions.length * 40 + 16));

  return (
    <div
      ref={ref}
      role="menu"
      tabIndex={-1}
      aria-label={label}
      className="fixed z-50 min-w-48 rounded-lg border border-border bg-popover p-1 shadow-2xl"
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
    </div>
  );
}
