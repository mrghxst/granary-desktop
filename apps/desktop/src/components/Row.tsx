import { ChevronLeft, ChevronRight } from "lucide-react";
import { useRef, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

/** A titled horizontal row that scrolls with wheel/trackpad, keyboard focus, or the arrow buttons. */
export function Row({ title, children }: { title: string; children: ReactNode }) {
  const { t } = useTranslation();
  const ref = useRef<HTMLDivElement>(null);
  const scroll = (dir: 1 | -1) => {
    const el = ref.current;
    if (!el) return;
    const reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    el.scrollBy({ left: dir * el.clientWidth * 0.85, behavior: reduce ? "auto" : "smooth" });
  };
  const arrow =
    "flex size-8 items-center justify-center rounded-full text-muted-foreground transition hover:bg-surface-hover hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring";
  return (
    <section className="flex flex-col gap-3" aria-label={title}>
      <div className="flex items-center justify-between">
        <h2 className="text-lg font-semibold tracking-tight">{title}</h2>
        <div className="flex gap-1">
          <button
            type="button"
            className={arrow}
            aria-label={t("browse.scrollLeft")}
            onClick={() => {
              scroll(-1);
            }}
          >
            <ChevronLeft className="size-5" />
          </button>
          <button
            type="button"
            className={arrow}
            aria-label={t("browse.scrollRight")}
            onClick={() => {
              scroll(1);
            }}
          >
            <ChevronRight className="size-5" />
          </button>
        </div>
      </div>
      <div ref={ref} className="scrollbar-none -mx-1 flex gap-4 overflow-x-auto px-1 pt-1 pb-2">
        {children}
      </div>
    </section>
  );
}
