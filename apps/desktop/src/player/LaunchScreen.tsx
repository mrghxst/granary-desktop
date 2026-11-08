import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { artwork } from "../lib/catalog";
import type { Launch } from "./context";

const FADE_MS = 450;

/**
 * Full-screen title card from Play until the first frame: the page is transparent over the native video window by
 * then, so without it the user sees an empty window. Fades out instead of cutting to the picture.
 */
export function LaunchScreen({ launch }: { launch: Launch | null }) {
  const { t } = useTranslation();
  // Keep the last launch while fading out (state derived during render, cleared by the fade timer).
  const [shown, setShown] = useState<Launch | null>(launch);
  const [logoFailed, setLogoFailed] = useState(false);
  if (launch && launch !== shown) setShown(launch);
  const leaving = launch === null && shown !== null;

  useEffect(() => {
    if (!leaving) return;
    const id = setTimeout(() => {
      setShown(null);
      setLogoFailed(false);
    }, FADE_MS);
    return () => {
      clearTimeout(id);
    };
  }, [leaving]);

  if (!shown) return null;
  const backdrop = artwork(shown.serverId, shown.backdrop);
  const logo = artwork(shown.serverId, shown.logo);

  return (
    <div
      role="status"
      aria-live="polite"
      aria-label={t("player.starting", { title: shown.title ?? "" })}
      data-testid="launch-screen"
      className={`fixed inset-0 z-[60] overflow-hidden bg-black text-white transition-opacity ease-out ${
        leaving ? "pointer-events-none opacity-0" : "opacity-100"
      }`}
      style={{ transitionDuration: `${String(FADE_MS)}ms` }}
    >
      {backdrop ? (
        <img
          src={backdrop}
          alt=""
          className="absolute inset-0 size-full animate-[launch-drift_14s_ease-out_forwards] object-cover opacity-0"
        />
      ) : null}
      <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_center,transparent_0%,rgba(0,0,0,0.55)_60%,#000_100%)]" />
      <div className="absolute inset-x-0 bottom-0 h-2/3 bg-gradient-to-t from-black via-black/70 to-transparent" />

      <div className="absolute inset-x-0 bottom-0 flex flex-col gap-5 p-10 lg:p-16">
        {logo && !logoFailed ? (
          <img
            src={logo}
            alt={shown.title ?? ""}
            className="max-h-32 max-w-md animate-[launch-rise_700ms_ease-out_both] object-contain object-left drop-shadow-2xl"
            onError={() => {
              setLogoFailed(true);
            }}
          />
        ) : shown.title ? (
          <h2 className="max-w-3xl animate-[launch-rise_700ms_ease-out_both] text-4xl font-bold tracking-tight lg:text-6xl">
            {shown.title}
          </h2>
        ) : null}
        {shown.subtitle ? (
          <p className="animate-[launch-rise_700ms_120ms_ease-out_both] text-lg text-white/75">{shown.subtitle}</p>
        ) : null}
        <div className="flex max-w-md flex-col gap-2.5">
          <div className="relative h-[2px] overflow-hidden rounded-full bg-white/15">
            <span className="absolute inset-y-0 left-0 w-1/3 animate-[launch-scan_1.4s_cubic-bezier(0.65,0,0.35,1)_infinite] rounded-full bg-white" />
          </div>
          <span className="text-xs font-medium tracking-[0.2em] text-white/50 uppercase">{t("player.preparing")}</span>
        </div>
      </div>
    </div>
  );
}
