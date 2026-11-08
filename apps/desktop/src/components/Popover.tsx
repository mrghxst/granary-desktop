import { Check, ChevronDown } from "lucide-react";
import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import type { TrackInfo } from "../lib/tracks";

/** A trigger with a panel below it; Escape and outside clicks close it. */
export function Popover({
  trigger,
  label,
  children,
  align = "start",
  className = "",
}: {
  trigger: (open: boolean) => ReactNode;
  label: string;
  children: (close: () => void) => ReactNode;
  align?: "start" | "end";
  className?: string;
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement>(null);
  const id = useId();
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    const onDown = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("mousedown", onDown);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("mousedown", onDown);
    };
  }, [open]);
  return (
    <div ref={ref} className={`relative ${className}`}>
      <button
        type="button"
        aria-haspopup="true"
        aria-expanded={open}
        aria-controls={id}
        aria-label={label}
        onClick={() => {
          setOpen((o) => !o);
        }}
        className="rounded-full focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
      >
        {trigger(open)}
      </button>
      {open ? (
        <div
          id={id}
          className={`absolute top-full z-50 mt-2 w-max max-w-[min(34rem,calc(100vw-3rem))] min-w-64 overflow-hidden rounded-xl border border-border bg-popover/95 p-1 shadow-2xl backdrop-blur-xl ${
            align === "end" ? "right-0" : "left-0"
          }`}
        >
          {children(() => {
            setOpen(false);
          })}
        </div>
      ) : null}
    </div>
  );
}

/** A selectable track row: language as headline, the track title below, badges right. */
export function TrackOption({
  info,
  selected,
  onSelect,
}: {
  info: TrackInfo;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitemradio"
      aria-checked={selected}
      onClick={onSelect}
      className={`flex w-full items-center gap-3 rounded-lg px-3 py-2 text-left transition hover:bg-surface-hover focus-visible:bg-surface-hover focus-visible:outline-none ${
        selected ? "bg-surface-hover/60" : ""
      }`}
    >
      <Check className={`size-4 shrink-0 ${selected ? "opacity-100" : "opacity-0"}`} />
      <span className="min-w-0 flex-1">
        <span className="block truncate text-sm font-semibold">{info.headline}</span>
        {info.detail ? <span className="block truncate text-xs text-muted-foreground">{info.detail}</span> : null}
      </span>
      {info.badges.length > 0 ? (
        <span className="flex shrink-0 flex-wrap justify-end gap-1">
          {info.badges.map((b) => (
            <Badge key={b}>{b}</Badge>
          ))}
        </span>
      ) : null}
    </button>
  );
}

export function Badge({ children, strong = false }: { children: ReactNode; strong?: boolean }) {
  return (
    <span
      className={`rounded-[5px] border px-1.5 py-px text-[10px] font-semibold tracking-wide uppercase ${
        strong ? "border-foreground/70 text-foreground" : "border-border text-muted-foreground"
      }`}
    >
      {children}
    </span>
  );
}

/** Closed state of a picker: a pill with a small caption and the current value. */
export function PickerPill({ caption, value, open }: { caption: string; value: string; open: boolean }) {
  return (
    <span className="flex items-center gap-2 rounded-full border border-border bg-black/30 py-1.5 pr-3 pl-4 text-sm backdrop-blur transition hover:border-foreground/40">
      <span className="text-xs font-medium text-muted-foreground">{caption}</span>
      <span className="max-w-56 truncate font-medium">{value}</span>
      <ChevronDown className={`size-3.5 text-muted-foreground transition ${open ? "rotate-180" : ""}`} />
    </span>
  );
}
