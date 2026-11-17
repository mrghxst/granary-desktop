import { Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { catalog, type FileMarkers, type MarkerKind } from "../lib/catalog";
import { errorMessage } from "../lib/errors";
import { MARKER_KINDS, markerEdits, markerFields, type MarkerFields } from "../lib/markers";
import { Alert, Button, Card } from "./ui";

/**
 * Type the intro, recap, credits and preview times of a movie or episode, as the web app's marker editor does
 * (`silo-server/web/src/components/markers/MarkerEditor.tsx@749db96a`). Saved segments are stored as manual ones and
 * used by every app from the next playback on.
 */
export function MarkerEditor({
  serverId,
  itemId,
  onClose,
}: {
  serverId: string;
  itemId: string;
  onClose: (saved: boolean) => void;
}) {
  const { t } = useTranslation();
  const [data, setData] = useState<FileMarkers | null>(null);
  const [fields, setFields] = useState<MarkerFields | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    catalog
      .itemMarkers(serverId, itemId)
      .then((d) => {
        if (!alive) return;
        setData(d);
        setFields(markerFields(d));
      })
      .catch((e: unknown) => {
        if (alive) setError(errorMessage(t, e));
      });
    return () => {
      alive = false;
    };
  }, [serverId, itemId, t]);

  const label = (kind: MarkerKind) => t(`markers.${kind}`);
  const setField = (kind: MarkerKind, edge: "start" | "end", value: string) => {
    setFields((f) => (f ? { ...f, [kind]: { ...f[kind], [edge]: value } } : f));
    setError(null);
  };
  const save = () => {
    if (!data || !fields) return;
    const edits = markerEdits(fields, data);
    if (!edits.ok) {
      setError(t(`markers.${edits.problem}`, { kind: label(edits.kind) }));
      return;
    }
    if (Object.keys(edits.set).length === 0 && edits.clear.length === 0) {
      onClose(false);
      return;
    }
    setBusy(true);
    setError(null);
    catalog
      .itemMarkersSave(serverId, itemId, edits.set, edits.clear)
      .then(() => {
        onClose(true);
      })
      .catch((e: unknown) => {
        setError(errorMessage(t, e));
        setBusy(false);
      });
  };

  return (
    <Card className="flex flex-col gap-4">
      <div>
        <h2 className="text-lg font-semibold">{t("markers.title")}</h2>
        <p className="text-sm text-muted-foreground">{t("markers.hint")}</p>
      </div>
      {fields === null && !error ? <p className="text-muted-foreground">{t("status.loading")}</p> : null}
      {fields ? (
        <div className="grid grid-cols-[6rem_1fr_1fr_auto] items-center gap-2">
          {MARKER_KINDS.map((kind) => (
            <div key={kind} className="contents">
              <span className="text-sm font-medium">{label(kind)}</span>
              {(["start", "end"] as const).map((edge) => (
                <input
                  key={edge}
                  aria-label={t(edge === "start" ? "markers.startOf" : "markers.endOf", { kind: label(kind) })}
                  placeholder={t(edge === "start" ? "markers.startPlaceholder" : "markers.endPlaceholder")}
                  value={fields[kind][edge]}
                  inputMode="numeric"
                  onChange={(e) => {
                    setField(kind, edge, e.target.value);
                  }}
                  className="h-9 rounded-md border border-border bg-background px-3 font-mono text-sm tabular-nums focus-visible:outline-2 focus-visible:outline-ring"
                />
              ))}
              <button
                type="button"
                aria-label={t("markers.clearOf", { kind: label(kind) })}
                title={t("markers.clearOf", { kind: label(kind) })}
                onClick={() => {
                  setField(kind, "start", "");
                  setField(kind, "end", "");
                }}
                className="rounded-md p-2 text-muted-foreground hover:bg-surface-hover hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring"
              >
                <Trash2 className="size-4" />
              </button>
            </div>
          ))}
        </div>
      ) : null}
      {error ? <Alert>{error}</Alert> : null}
      <div className="flex justify-end gap-2">
        <Button
          variant="ghost"
          onClick={() => {
            onClose(false);
          }}
        >
          {t("markers.cancel")}
        </Button>
        <Button onClick={save} disabled={busy || fields === null}>
          {t("markers.save")}
        </Button>
      </div>
    </Card>
  );
}
