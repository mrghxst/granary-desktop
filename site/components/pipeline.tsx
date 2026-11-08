"use client";
import { useEffect, useRef, useState } from "react";

const STEPS = [
  {
    k: "01",
    title: "Negotiate",
    body: "The server picks direct play, remux or transcode for this exact file, monitor and audio setup. Direct play is the default for remuxes.",
    code: "startPlayback → PlaybackDecision{ delivery: original_http }",
  },
  {
    k: "02",
    title: "Relay",
    body: "A loopback relay on 127.0.0.1 forwards the stream with your session. mpv never sees a token; the UI never does either.",
    code: "http://127.0.0.1:51855/r/<128-bit route>/stream",
  },
  {
    k: "03",
    title: "Render",
    body: "libmpv with gpu-next on a D3D11 flip swapchain, d3d11va decoding and libplacebo tone mapping to the panel's real peak brightness.",
    code: "vo=gpu-next  gpu-api=d3d11  hwdec=auto-safe  target-peak=<DXGI>",
  },
  {
    k: "04",
    title: "Display",
    body: "HDR switches on only when it should, the refresh rate follows the content or stays put on VRR, per monitor.",
    code: "refresh: VRR → fits → exact multiple → display-resample",
  },
  {
    k: "05",
    title: "Restore",
    body: "Every display change is written to a restore marker first. Stop, crash or power loss: the next start puts your desktop back.",
    code: "marker.json → restore(hdr, refresh) → ok=true",
  },
] as const;

/** Sticky explainer: the step nearest the middle of the viewport lights up. */
export function Pipeline() {
  const [active, setActive] = useState(0);
  const refs = useRef<(HTMLLIElement | null)[]>([]);

  useEffect(() => {
    if (typeof IntersectionObserver === "undefined") return;
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (e.isIntersecting) setActive(Number((e.target as HTMLElement).dataset.i));
        }
      },
      { rootMargin: "-45% 0px -45% 0px" },
    );
    for (const el of refs.current) if (el) io.observe(el);
    return () => io.disconnect();
  }, []);

  return (
    <div className="grid gap-12 lg:grid-cols-[1fr_1.1fr]">
      <div className="lg:sticky lg:top-28 lg:self-start">
        <p className="font-mono text-xs tracking-[0.25em] text-white/40 uppercase">How playback works</p>
        <h2 className="mt-4 text-4xl font-semibold tracking-tight text-white md:text-5xl">
          From Play to first frame,
          <br />
          <span className="text-white/45">nothing in between.</span>
        </h2>
        <div className="mt-10 overflow-hidden rounded-xl border border-white/10 bg-white/[0.02] font-mono text-[13px]">
          <div className="flex items-center gap-2 border-b border-white/10 px-4 py-2.5 text-white/40">
            <span className="size-2 rounded-full bg-white/20" />
            granary · playback
          </div>
          <div className="px-4 py-4">
            {STEPS.map((s, i) => (
              <div
                key={s.k}
                className={`flex gap-3 py-1 transition-all duration-500 ${
                  i === active ? "text-white" : i < active ? "text-white/45" : "text-white/15"
                }`}
              >
                <span className="text-white/30">{s.k}</span>
                <span className="truncate">{s.code}</span>
              </div>
            ))}
          </div>
          <div className="h-0.5 bg-white/5">
            <div
              className="h-full bg-white transition-[width] duration-700 ease-out"
              style={{ width: `${((active + 1) / STEPS.length) * 100}%` }}
            />
          </div>
        </div>
      </div>
      <ol className="flex flex-col">
        {STEPS.map((s, i) => (
          <li
            key={s.k}
            data-i={i}
            ref={(el) => {
              refs.current[i] = el;
            }}
            className={`border-l py-14 pl-8 transition-colors duration-500 ${
              i === active ? "border-white" : "border-white/10"
            }`}
          >
            <span className="font-mono text-sm text-white/35">{s.k}</span>
            <h3
              className={`mt-2 text-2xl font-semibold tracking-tight transition-colors duration-500 ${
                i === active ? "text-white" : "text-white/40"
              }`}
            >
              {s.title}
            </h3>
            <p className="mt-3 max-w-md leading-relaxed text-white/55">{s.body}</p>
          </li>
        ))}
      </ol>
    </div>
  );
}
