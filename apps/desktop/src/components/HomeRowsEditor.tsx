import { ChevronDown, ChevronUp, Eye, EyeOff } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { catalog, type HomeRows } from "../lib/catalog";
import { errorMessage } from "../lib/errors";
import { buildHomeOverrides } from "../lib/homeRows";
import { Alert, Button, Card } from "./ui";

/**
 * Hide, show and reorder the home page's rows for this profile, as Settings > Home Screen does in the web app.
 * Renaming rows and building new ones stay in the web app; whatever it saved is kept.
 */
export function HomeRowsEditor({ serverId, onClose }: { serverId: string; onClose: (changed: boolean) => void }) {
  const { t } = useTranslation();
  const [page, setPage] = useState<HomeRows | null>(null);
  const [order, setOrder] = useState<string[]>([]);
  const [hidden, setHidden] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    catalog
      .homeRows(serverId)
      .then((p) => {
        if (!alive) return;
        setPage(p);
        setOrder(p.rows.map((r) => r.id));
        setHidden(new Set(p.rows.filter((r) => r.hidden).map((r) => r.id)));
      })
      .catch((e: unknown) => {
        if (alive) setError(errorMessage(t, e));
      });
    return () => {
      alive = false;
    };
  }, [serverId, t]);

  const byId = new Map(page?.rows.map((r) => [r.id, r]));
  const move = (index: number, by: -1 | 1) => {
    setOrder((o) => {
      const next = [...o];
      const [id] = next.splice(index, 1);
      if (id !== undefined) next.splice(index + by, 0, id);
      return next;
    });
  };
  const toggle = (id: string) => {
    setHidden((h) => {
      const next = new Set(h);
      if (!next.delete(id)) next.add(id);
      return next;
    });
  };
  const run = async (write: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await write();
      onClose(true);
    } catch (e) {
      setError(errorMessage(t, e));
      setBusy(false);
    }
  };
  const save = () => {
    if (!page) return;
    void run(() => catalog.homeRowsSave(serverId, buildHomeOverrides(page.rows, page.overrides, { order, hidden })));
  };

  return (
    <Card className="flex flex-col gap-4">
      <div>
        <h2 className="text-lg font-semibold">{t("homeRows.title")}</h2>
        <p className="text-sm text-muted-foreground">{t("homeRows.hint")}</p>
      </div>
      {error ? <Alert>{error}</Alert> : null}
      {page === null && !error ? <p className="text-muted-foreground">{t("status.loading")}</p> : null}
      {page !== null ? (
        <ol className="flex flex-col gap-2">
          {order.map((id, index) => {
            const row = byId.get(id);
            if (!row) return null;
            const title = row.title || row.default_title || row.section_type;
            const off = hidden.has(id);
            return (
              <li key={id} className="flex items-center gap-2 rounded-lg border border-border bg-background px-3 py-2">
                <span className={`min-w-0 flex-1 truncate ${off ? "text-muted-foreground line-through" : ""}`}>
                  {title}
                  {row.featured ? (
                    <span className="ml-2 text-xs text-muted-foreground">{t("homeRows.featured")}</span>
                  ) : null}
                </span>
                <Button
                  variant="ghost"
                  className="px-2"
                  aria-label={t("homeRows.up", { title })}
                  disabled={busy || index === 0}
                  onClick={() => {
                    move(index, -1);
                  }}
                >
                  <ChevronUp className="size-4" />
                </Button>
                <Button
                  variant="ghost"
                  className="px-2"
                  aria-label={t("homeRows.down", { title })}
                  disabled={busy || index === order.length - 1}
                  onClick={() => {
                    move(index, 1);
                  }}
                >
                  <ChevronDown className="size-4" />
                </Button>
                <Button
                  variant="ghost"
                  className="px-2"
                  role="switch"
                  aria-checked={!off}
                  aria-label={t("homeRows.show", { title })}
                  disabled={busy}
                  onClick={() => {
                    toggle(id);
                  }}
                >
                  {off ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
                </Button>
              </li>
            );
          })}
        </ol>
      ) : null}
      <div className="flex flex-wrap gap-2">
        <Button disabled={busy || page === null} onClick={save}>
          {t("homeRows.save")}
        </Button>
        <Button
          variant="secondary"
          disabled={busy}
          onClick={() => {
            onClose(false);
          }}
        >
          {t("homeRows.cancel")}
        </Button>
        <Button
          variant="ghost"
          className="ml-auto"
          disabled={busy || page === null}
          onClick={() => void run(() => catalog.homeRowsReset(serverId))}
        >
          {t("homeRows.reset")}
        </Button>
      </div>
    </Card>
  );
}
