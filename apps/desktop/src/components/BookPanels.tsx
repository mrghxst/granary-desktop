import { BookOpen, Check } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";
import { books, fileFormat, readerFile } from "../lib/books";
import type { CatalogItemDetail } from "../lib/catalog";
import { retryLoads, useRefreshTick } from "../lib/events";
import { useLoad } from "../lib/useLoad";
import { Alert } from "./ui";

type MangaChapter = NonNullable<CatalogItemDetail["manga"]>["chapters"][number];

const readPath = (serverId: string, contentId: string) => `/servers/${serverId}/read/${encodeURIComponent(contentId)}`;

function ReadButton({ progress, label, onClick }: { progress: number; label: string; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="relative isolate inline-flex h-12 items-center gap-2.5 overflow-hidden rounded-full bg-primary pr-6 pl-5 font-semibold text-primary-foreground shadow-lg transition hover:scale-[1.02] focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring motion-reduce:transition-none"
    >
      {progress > 0 ? (
        <span
          aria-hidden
          className="absolute inset-y-0 left-0 -z-10 bg-black/15"
          style={{ width: `${String(progress * 100)}%` }}
        />
      ) : null}
      <BookOpen className="size-5" />
      {label}
    </button>
  );
}

const names = (l: { name: string }[] | undefined) => (l ?? []).map((p) => p.name).join(", ");

/** Read / Continue reading for an ebook, with its authors, format and place. */
export function ReadPanel({ serverId, item, extra }: { serverId: string; item: CatalogItemDetail; extra?: ReactNode }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const refresh = useRefreshTick();
  const { data: saved, error } = useLoad(
    `${serverId}/${item.content_id}/ebook-progress`,
    () => books.progress(serverId, item.content_id),
    refresh,
  );
  const file = readerFile(item.versions);
  if (!file) return <Alert tone="warning">{t("reader.noFile")}</Alert>;
  const progress = saved?.progress ?? 0;
  const authors = names(item.ebook?.authors);
  return (
    <div className="flex flex-col gap-3">
      {authors ? <p className="text-sm text-foreground/80">{authors}</p> : null}
      <div className="flex flex-wrap items-center gap-3">
        <ReadButton
          progress={progress}
          label={
            progress > 0 && progress < 1
              ? `${t("reader.continue")} · ${String(Math.round(progress * 100))}%`
              : t("reader.read")
          }
          onClick={() => void navigate(readPath(serverId, item.content_id))}
        />
        {extra}
      </div>
      <p className="text-xs text-muted-foreground">
        {[fileFormat(file).toUpperCase(), item.ebook?.publisher, progress >= 1 ? t("reader.finished") : null]
          .filter(Boolean)
          .join(" · ")}
      </p>
      {error ? <Alert onRetry={retryLoads}>{t("reader.progressUnavailable")}</Alert> : null}
    </div>
  );
}

/** The chapter to open next: the first one not finished (the server's `read`, or full progress). */
function nextChapter(chapters: readonly MangaChapter[]): MangaChapter | undefined {
  return chapters.find((c) => !c.read && (c.progress ?? 0) < 0.999) ?? chapters[0];
}

/** Read / Continue for a manga series: opens the next unfinished chapter. */
export function MangaPanel({
  serverId,
  item,
  extra,
}: {
  serverId: string;
  item: CatalogItemDetail;
  extra?: ReactNode;
}) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const chapters = item.manga?.chapters ?? [];
  const up = nextChapter(chapters);
  if (!up) return <Alert tone="warning">{t("reader.noChapters")}</Alert>;
  const started = chapters.some((c) => c.read || (c.progress ?? 0) > 0);
  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-3">
        <ReadButton
          progress={0}
          label={started ? `${t("reader.continue")} · ${up.title}` : t("reader.read")}
          onClick={() => void navigate(readPath(serverId, up.content_id))}
        />
        {extra}
      </div>
      <p className="text-xs text-muted-foreground">{t("reader.chapterCount", { count: chapters.length })}</p>
    </div>
  );
}

/** A manga series' chapters, by volume, with each one's reading state. */
export function MangaChapterList({ serverId, item }: { serverId: string; item: CatalogItemDetail }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const chapters = item.manga?.chapters ?? [];
  if (chapters.length === 0) return null;
  const groups: { volume: string; chapters: MangaChapter[] }[] = [];
  for (const c of chapters) {
    const volume = c.volume?.trim() ?? "";
    const last = groups[groups.length - 1];
    if (last && last.volume === volume) last.chapters.push(c);
    else groups.push({ volume, chapters: [c] });
  }
  const volumeLabel = (v: string) => {
    const m = /^v?(\d+)$/i.exec(v);
    return m ? t("reader.volume", { n: Number(m[1]) }) : v;
  };
  return (
    <section className="flex flex-col gap-4">
      <h2 className="text-xl font-semibold tracking-tight">{t("reader.chapters")}</h2>
      {groups.map((g, gi) => (
        <div key={`${g.volume}-${String(gi)}`} className="flex flex-col gap-1">
          {g.volume ? <h3 className="text-sm font-semibold text-muted-foreground">{volumeLabel(g.volume)}</h3> : null}
          <ul className="flex flex-col divide-y divide-border/50 overflow-hidden rounded-xl border border-border/60 bg-card">
            {g.chapters.map((c) => {
              const p = c.read ? 1 : (c.progress ?? 0);
              return (
                <li key={c.content_id}>
                  <button
                    type="button"
                    onClick={() => void navigate(readPath(serverId, c.content_id))}
                    className="flex w-full items-center gap-3 px-4 py-3 text-left hover:bg-white/5 focus-visible:outline-2 focus-visible:outline-ring"
                  >
                    <span className="min-w-0 flex-1 truncate">{c.title}</span>
                    {p >= 0.999 ? (
                      <span className="inline-flex items-center gap-1 text-xs text-muted-foreground">
                        <Check className="size-4" />
                        {t("reader.finished")}
                      </span>
                    ) : p > 0 ? (
                      <span className="flex items-center gap-2 text-xs text-muted-foreground tabular-nums">
                        <span className="h-1 w-16 overflow-hidden rounded-full bg-white/10">
                          <span className="block h-full bg-primary" style={{ width: `${String(p * 100)}%` }} />
                        </span>
                        {Math.round(p * 100)}%
                      </span>
                    ) : null}
                  </button>
                </li>
              );
            })}
          </ul>
        </div>
      ))}
    </section>
  );
}
