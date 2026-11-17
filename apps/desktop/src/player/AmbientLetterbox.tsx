import { useEffect, useRef, useState } from "react";
import { artwork } from "../lib/catalog";
import { providerTile, type PreviewProvider } from "../lib/previews";
import { useLocalPreview } from "./useLocalPreview";
import { pictureBand } from "../lib/letterbox";

/** Media seconds between two pictures for the bands: the glow follows the scene, not every frame. */
const STEP_SECONDS = 10;
/** Share of the picture's height each band samples from the edge it touches (as the web app). */
const EDGE_SHARE = 0.3;

/**
 * Lets the picture's colour spill into the black bars above and below a wide film while the controls show, as the
 * web player does (`silo-server/web/src/player/components/AmbientLetterbox.tsx@749db96a`). mpv draws the video in its
 * own window, so the bands take the seek preview of the current spot instead (server trickplay, Seekr or a locally
 * decoded keyframe): each band is a tiny, blurred copy of the picture strip it touches, redrawn every few seconds of
 * video. Pixels are only drawn, never read back.
 */
export function AmbientLetterbox({
  serverId,
  provider,
  position,
  duration,
  aspect,
  active,
}: {
  serverId: string;
  provider: PreviewProvider;
  position: number;
  duration: number;
  /** Display aspect of the video (width / height), 0 while unknown. */
  aspect: number;
  /** The controls show and the pointer is not on the seek bar (whose previews take the decoder then). */
  active: boolean;
}) {
  const [size, setSize] = useState({ width: window.innerWidth, height: window.innerHeight });
  useEffect(() => {
    const onResize = () => {
      setSize({ width: window.innerWidth, height: window.innerHeight });
    };
    window.addEventListener("resize", onResize);
    return () => {
      window.removeEventListener("resize", onResize);
    };
  }, []);
  const band = pictureBand(size.width, size.height, aspect);
  const on = active && band !== null;
  const spot = Math.max(0, Math.floor(position / STEP_SECONDS) * STEP_SECONDS);
  const tile = on ? providerTile(provider, spot) : undefined;
  const local = useLocalPreview(on && provider.kind === "local", on ? spot : null, duration);
  const src = tile
    ? tile.server === "file"
      ? `${artwork(serverId, tile.url) ?? ""}&p=1`
      : (artwork(tile.server, tile.url) ?? "")
    : (local?.url ?? "");
  const top = useRef<HTMLCanvasElement>(null);
  const bottom = useRef<HTMLCanvasElement>(null);
  const crop = tile ? `${String(tile.x)},${String(tile.y)},${String(tile.width)},${String(tile.height)}` : "";
  const [drawn, setDrawn] = useState(false);

  useEffect(() => {
    if (!on || !src) return;
    let live = true;
    const img = new Image();
    img.onload = () => {
      if (!live) return;
      const [x, y, w, h] = crop ? crop.split(",").map(Number) : [0, 0, img.naturalWidth, img.naturalHeight];
      if (x === undefined || y === undefined || !w || !h) return;
      const strip = h * EDGE_SHARE;
      const paint = (canvas: HTMLCanvasElement | null, sy: number) => {
        const c = canvas?.getContext("2d");
        if (!canvas || !c) return;
        c.drawImage(img, x, sy, w, strip, 0, 0, canvas.width, canvas.height);
      };
      paint(top.current, y);
      paint(bottom.current, y + h - strip);
      setDrawn(true);
    };
    img.src = src;
    return () => {
      live = false;
    };
  }, [on, src, crop]);

  if (!band) return null;
  const shown = on && drawn;
  const style = "pointer-events-none absolute inset-x-0 overflow-hidden transition-opacity duration-700";
  const canvas = "block size-full scale-x-[1.3] scale-y-[-1.3] blur-[28px] saturate-[1.2] brightness-50";
  return (
    <>
      <div
        aria-hidden="true"
        className={`${style} [mask-image:linear-gradient(to_top,#000,rgb(0_0_0/0.4)_50%,transparent)]`}
        style={{ top: 0, height: band.top, opacity: shown ? 1 : 0 }}
      >
        <canvas ref={top} width={24} height={8} className={canvas} />
      </div>
      <div
        aria-hidden="true"
        className={`${style} [mask-image:linear-gradient(to_bottom,#000,rgb(0_0_0/0.4)_50%,transparent)]`}
        style={{ top: band.bottom, height: Math.max(0, size.height - band.bottom), opacity: shown ? 1 : 0 }}
      >
        <canvas ref={bottom} width={24} height={8} className={canvas} />
      </div>
    </>
  );
}
