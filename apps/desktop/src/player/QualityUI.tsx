import { ArrowRight, Check, Loader2, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { PlanInfo } from "../lib/player";
import {
  audioLine,
  convertsVideo,
  deliveredSummary,
  explainPlan,
  formatKbps,
  methodName,
  playMethod,
  resolutionLabel,
  streamLine,
  type PlayMethod,
  type Reason,
} from "../lib/quality";

/** One colour per delivery family, the same everywhere the method shows. */
const METHOD_STYLE: Record<PlayMethod, { dot: string; chip: string; text: string }> = {
  direct_play: {
    dot: "bg-emerald-400",
    chip: "border-emerald-400/40 bg-emerald-400/15",
    text: "text-emerald-300",
  },
  direct_stream: {
    dot: "bg-sky-400",
    chip: "border-sky-400/40 bg-sky-400/15",
    text: "text-sky-300",
  },
  transcode: {
    dot: "bg-amber-400",
    chip: "border-amber-400/45 bg-amber-400/15",
    text: "text-amber-300",
  },
};

/**
 * The play method as a coloured chip: green Direct Play, blue Direct Stream, amber Transcoding, with what the player
 * receives. The top bar shows it whenever the controls are up, so it is never a guess whether the server converts.
 */
export function PlayMethodBadge({
  plan,
  busy = false,
  onClick,
}: {
  plan: PlanInfo;
  busy?: boolean;
  onClick?: () => void;
}) {
  const { t } = useTranslation();
  const m = playMethod(plan);
  const s = METHOD_STYLE[m];
  const summary = deliveredSummary(plan);
  const content = (
    <>
      {busy ? (
        <Loader2 className="size-3.5 animate-spin text-white/80" aria-hidden="true" />
      ) : (
        <span aria-hidden="true" className={`size-2 shrink-0 rounded-full ${s.dot}`} />
      )}
      <span className={`font-semibold ${s.text}`}>{busy ? t("quality.switching") : methodName(t, plan)}</span>
      {summary ? <span className="text-white/75 tabular-nums">{summary}</span> : null}
    </>
  );
  const cls = `flex shrink-0 items-center gap-2 rounded-full border px-3 py-1 text-sm backdrop-blur-md ${s.chip}`;
  if (!onClick) return <div className={cls}>{content}</div>;
  return (
    <button
      type="button"
      className={`${cls} transition hover:brightness-125 focus-visible:outline-2 focus-visible:outline-white/70`}
      title={t("quality.open")}
      aria-label={`${methodName(t, plan)}${summary ? ` · ${summary}` : ""}. ${t("quality.open")}`}
      onClick={onClick}
    >
      {content}
    </button>
  );
}

/** The toolbar's quality button: the resolution playing, framed in the method's colour. */
export function QualityButton({ plan, active, onClick }: { plan: PlanInfo; active: boolean; onClick: () => void }) {
  const { t } = useTranslation();
  const m = playMethod(plan);
  const res =
    resolutionLabel(plan.delivered.width, plan.delivered.height) ||
    (plan.quality_preference === "auto" ? "Auto" : plan.quality_preference);
  const label = `${t("quality.title")}: ${methodName(t, plan)}${res ? ` · ${res}` : ""}`;
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      aria-pressed={active || undefined}
      onClick={onClick}
      className={`mx-1 flex h-7 min-w-11 items-center justify-center gap-1.5 rounded-md border px-2 text-xs font-bold tracking-wide transition hover:bg-white/15 focus-visible:outline-2 focus-visible:outline-ring ${
        METHOD_STYLE[m].chip
      } ${active ? "ring-2 ring-white/60" : ""}`}
    >
      <span aria-hidden="true" className={`size-1.5 rounded-full ${METHOD_STYLE[m].dot}`} />
      <span className="text-white">{res || "—"}</span>
    </button>
  );
}

function ReasonText({ r }: { r: Reason | string }) {
  const { t } = useTranslation();
  return <>{typeof r === "string" ? r : t(r.key, r.values ?? {})}</>;
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="grid grid-cols-[5.5rem_1fr] gap-3 py-1">
      <dt className="text-white/50">{label}</dt>
      <dd className="min-w-0 text-white/90">{children}</dd>
    </div>
  );
}

/** What plays now, how, and why: the head of the quality menu. */
export function NowPlaying({ plan }: { plan: PlanInfo }) {
  const { t } = useTranslation();
  const m = playMethod(plan);
  const s = METHOD_STYLE[m];
  const { reasons, changes, warnings } = explainPlan(plan);
  const video = convertsVideo(plan);
  const sentence =
    m === "direct_play"
      ? t("quality.say.direct_play")
      : m === "direct_stream"
        ? t("quality.say.direct_stream")
        : video
          ? t("quality.say.transcode")
          : t("quality.say.transcodeAudio");
  const original = streamLine(plan.source);
  const playing = streamLine(plan.delivered, plan.delivered_kbps);
  return (
    <div className={`mx-1 mb-2 rounded-xl border p-3 ${s.chip}`}>
      <div className="flex items-center gap-2">
        <span aria-hidden="true" className={`size-2.5 rounded-full ${s.dot}`} />
        <span className={`text-base font-semibold ${s.text}`}>{methodName(t, plan)}</span>
      </div>
      <p className="mt-1 text-[13px] leading-snug text-white/80">{sentence}</p>
      <dl className="mt-2 text-[13px]">
        {m === "direct_play" ? (
          <Row label={t("quality.playing")}>
            <div className="truncate">{original || "—"}</div>
            {audioLine(plan.source) ? <div className="truncate text-white/60">{audioLine(plan.source)}</div> : null}
          </Row>
        ) : (
          <>
            <Row label={t("quality.original")}>
              <div className="truncate">{original || "—"}</div>
              {audioLine(plan.source) ? <div className="truncate text-white/60">{audioLine(plan.source)}</div> : null}
            </Row>
            <Row label={t("quality.playing")}>
              <div className="truncate">{playing || "—"}</div>
              {audioLine(plan.delivered) ? (
                <div className="truncate text-white/60">{audioLine(plan.delivered)}</div>
              ) : null}
            </Row>
          </>
        )}
      </dl>
      <div className="mt-2 border-t border-white/10 pt-2 text-[13px]">
        <div className="mb-1 text-xs font-semibold tracking-wide text-white/50 uppercase">{t("quality.why.title")}</div>
        <ul className="space-y-1 text-white/85">
          {reasons.map((r) => (
            <li key={r.key}>
              <ReasonText r={r} />
            </li>
          ))}
        </ul>
        {changes.length > 0 ? (
          <ul className="mt-2 space-y-1 text-white/75">
            {changes.map((c, i) => (
              <li key={`${c.key}-${String(i)}`} className="flex items-start gap-2">
                <ArrowRight className="mt-0.5 size-3.5 shrink-0 text-white/40" aria-hidden="true" />
                <span>
                  <ReasonText r={c} />
                </span>
              </li>
            ))}
          </ul>
        ) : null}
        {warnings.length > 0 ? (
          <ul className="mt-2 space-y-1 text-amber-200/90">
            {warnings.map((w, i) => (
              <li key={i} className="flex items-start gap-2">
                <TriangleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden="true" />
                <span>
                  <ReasonText r={w} />
                </span>
              </li>
            ))}
          </ul>
        ) : null}
      </div>
    </div>
  );
}

function Pick({
  selected,
  disabled,
  onSelect,
  title,
  hint,
  tag,
}: {
  selected: boolean;
  disabled: boolean;
  onSelect: () => void;
  title: string;
  hint?: string;
  tag?: ReactNode;
}) {
  return (
    <button
      type="button"
      role="menuitemradio"
      aria-checked={selected}
      disabled={disabled}
      onClick={onSelect}
      className={`flex w-full items-center gap-3 rounded-xl px-3 py-2 text-left transition-colors focus-visible:outline-2 focus-visible:outline-white/70 disabled:opacity-50 ${
        selected ? "bg-white/[0.12] text-white" : "text-white/85 hover:bg-white/[0.07] hover:text-white"
      }`}
    >
      <Check className={`size-4 shrink-0 transition-opacity ${selected ? "opacity-100" : "opacity-0"}`} />
      <span className="min-w-0 flex-1">
        <span className="block font-medium">{title}</span>
        {hint ? <span className="block truncate text-xs text-white/55">{hint}</span> : null}
      </span>
      {tag}
    </button>
  );
}

/**
 * The quality menu: what plays and why, then Auto and the server's rungs for this file. A pick replans this playback
 * only; the default lives in Settings.
 */
export function QualityPanel({
  plan,
  busy,
  inParty,
  onPick,
}: {
  plan: PlanInfo;
  busy: boolean;
  inParty: boolean;
  onPick: (label: string) => void;
}) {
  const { t } = useTranslation();
  const pref = plan.quality_preference || "auto";
  const auto = pref === "auto";
  const autoHint = plan.bandwidth_limit
    ? t("quality.autoLimited", { limit: formatKbps(plan.bandwidth_limit.kbps) })
    : plan.cap_kbps
      ? t("quality.autoCapped", { cap: formatKbps(plan.cap_kbps) })
      : t("quality.autoHint");
  const rungs = plan.available_qualities;
  return (
    <div className="text-sm">
      <NowPlaying plan={plan} />
      <div className="mt-3 mb-1 px-3 text-xs font-semibold tracking-wide text-white/50 uppercase">
        {t("quality.choose")}
      </div>
      {inParty ? (
        <p className="px-3 pb-2 text-[13px] text-white/60">{t("quality.party")}</p>
      ) : (
        <>
          <Pick
            selected={auto}
            disabled={busy}
            onSelect={() => {
              if (!auto) onPick("auto");
            }}
            title={t("quality.auto")}
            hint={autoHint}
            tag={
              auto && plan.active_quality ? (
                <span className="shrink-0 text-xs text-white/55">
                  {t("quality.nowRung", {
                    name: rungs.find((q) => q.label === plan.active_quality)?.display_name ?? plan.active_quality,
                  })}
                </span>
              ) : null
            }
          />
          {rungs.map((q) => (
            <Pick
              key={q.label}
              selected={pref === q.label}
              disabled={busy}
              onSelect={() => {
                if (pref !== q.label) onPick(q.label);
              }}
              title={q.display_name ?? q.label}
              hint={q.preserves_source ? t("quality.preservesSource") : t("quality.transcodes")}
              tag={
                <span className="shrink-0 text-xs text-white/55 tabular-nums">
                  {q.preserves_source ? t("quality.originalTag") : formatKbps(q.bitrate_kbps)}
                </span>
              }
            />
          ))}
          {rungs.length === 0 ? <p className="px-3 py-2 text-[13px] text-white/60">{t("quality.noRungs")}</p> : null}
          <p className="px-3 pt-2 pb-1 text-xs text-white/45">{t("quality.thisPlayback")}</p>
        </>
      )}
    </div>
  );
}

/** The buffering prompt ("ask") or the undo after an automatic lowering, with its countdown. */
export function BufferingPill({
  pill,
  onAct,
}: {
  pill: { kind: "prompt" | "undo"; remainingMs: number };
  onAct: (kind: "lower" | "keep" | "restore" | "back") => void;
}) {
  const { t } = useTranslation();
  const ratio = Math.max(0, Math.min(1, pill.remainingMs / 15_000));
  return (
    <div
      role="alertdialog"
      aria-label={t(`quality.pill.${pill.kind}Title`)}
      className="absolute right-8 bottom-32 flex w-96 flex-col gap-3 overflow-hidden rounded-xl border border-amber-400/30 bg-black/85 p-4 shadow-2xl backdrop-blur-md"
    >
      <div className="flex items-start gap-3">
        <TriangleAlert className="mt-0.5 size-5 shrink-0 text-amber-300" aria-hidden="true" />
        <div className="min-w-0">
          <div className="font-semibold">{t(`quality.pill.${pill.kind}Title`)}</div>
          <div className="mt-0.5 text-sm text-white/70">{t(`quality.pill.${pill.kind}Body`)}</div>
        </div>
      </div>
      <div className="flex gap-2">
        {pill.kind === "prompt" ? (
          <>
            <button
              type="button"
              className="flex-1 rounded-lg bg-primary px-3 py-2 text-sm font-medium text-primary-foreground"
              onClick={() => {
                onAct("lower");
              }}
            >
              {t("quality.pill.lower")}
            </button>
            <button
              type="button"
              className="flex-1 rounded-lg bg-white/15 px-3 py-2 text-sm"
              onClick={() => {
                onAct("keep");
              }}
            >
              {t("quality.pill.keep")}
            </button>
          </>
        ) : (
          <>
            <button
              type="button"
              className="flex-1 rounded-lg bg-white/15 px-3 py-2 text-sm font-medium"
              onClick={() => {
                onAct("restore");
              }}
            >
              {t("quality.pill.restore")}
            </button>
            <button
              type="button"
              className="flex-1 rounded-lg bg-white/15 px-3 py-2 text-sm"
              onClick={() => {
                onAct("back");
              }}
            >
              {t("quality.pill.ok")}
            </button>
          </>
        )}
      </div>
      <div aria-hidden="true" className="absolute right-0 bottom-0 left-0 h-0.5 bg-white/10">
        <div
          className="h-full bg-amber-300/80 transition-[width] duration-300"
          style={{ width: `${String(ratio * 100)}%` }}
        />
      </div>
    </div>
  );
}
