/* eslint-disable @next/next/no-img-element -- static export without image optimisation */
import {
  ArrowRight,
  AudioLines,
  Download,
  KeyRound,
  Languages,
  MonitorCheck,
  Rewind,
  ShieldCheck,
  Sparkles,
} from "lucide-react";
import Link from "next/link";
import type { ReactNode } from "react";
import { GithubIcon } from "@/components/github-icon";
import { Pipeline } from "@/components/pipeline";
import { Reveal } from "@/components/reveal";
import { Spotlight } from "@/components/spotlight";
import { asset, blob, releases, repo } from "@/lib/site";

const STACK = [
  "Tauri 2",
  "Rust 2024",
  "React 19",
  "libmpv",
  "libplacebo",
  "gpu-next",
  "Direct3D 11",
  "d3d11va",
  "WebView2",
  "HDR10",
  "Dolby Vision",
  "VRR",
  "WASAPI",
  "TrueHD · DTS-HD passthrough",
];

function Nav() {
  return (
    <header className="fixed inset-x-0 top-0 z-50 border-b border-white/[0.06] bg-black/60 backdrop-blur-xl">
      <nav className="mx-auto flex h-14 max-w-6xl items-center justify-between px-4 md:px-6">
        <Link href="/" className="flex items-center gap-2.5 font-semibold tracking-tight text-white">
          <img src={asset("/icon.svg")} alt="" width={24} height={24} />
          Granary
        </Link>
        <div className="flex items-center gap-1 text-sm">
          <a href="#features" className="hidden rounded-md px-3 py-1.5 text-white/60 hover:text-white sm:block">
            Features
          </a>
          <Link href="/docs" className="rounded-md px-3 py-1.5 text-white/60 hover:text-white">
            Docs
          </Link>
          <a href={repo} className="rounded-md px-3 py-1.5 text-white/60 hover:text-white" aria-label="GitHub">
            <GithubIcon className="size-4" />
          </a>
          <a
            href={releases}
            className="ml-2 rounded-full bg-white px-3.5 py-1.5 font-medium text-black transition hover:bg-white/85"
          >
            Download
          </a>
        </div>
      </nav>
    </header>
  );
}

function Hero() {
  return (
    <section className="relative overflow-hidden pt-36 pb-10 md:pt-44">
      <div className="grid-bg pointer-events-none absolute inset-0" />
      <div className="beam pointer-events-none absolute top-[-30%] left-1/2 size-[900px] -translate-x-1/2 rounded-full opacity-60" />
      <div className="relative mx-auto max-w-6xl px-4 text-center md:px-6">
        <Reveal>
          <a
            href={blob("CHANGELOG.md")}
            className="inline-flex items-center gap-2 rounded-full border border-white/10 bg-white/[0.03] px-3 py-1 text-xs text-white/70 backdrop-blur hover:border-white/25"
          >
            <span className="anim-pulse size-1.5 rounded-full bg-white" />
            Windows 11 · compatible with Silo media servers
            <ArrowRight className="size-3" />
          </a>
        </Reveal>
        <Reveal delay={80}>
          <h1 className="text-shine mx-auto mt-8 max-w-4xl text-5xl font-semibold tracking-[-0.04em] md:text-7xl">
            Your library, played the way it was mastered.
          </h1>
        </Reveal>
        <Reveal delay={160}>
          <p className="mx-auto mt-6 max-w-2xl text-lg leading-relaxed text-white/55">
            Granary is a native desktop client for your self-hosted media server. mpv and libplacebo do the playback,
            HDR lands correctly on OLED monitors, and every display change is put back when you stop.
          </p>
        </Reveal>
        <Reveal delay={240} className="mt-10 flex flex-wrap items-center justify-center gap-3">
          <a
            href={releases}
            className="inline-flex items-center gap-2 rounded-full bg-white px-5 py-2.5 text-sm font-medium text-black transition hover:bg-white/85"
          >
            <Download className="size-4" /> Download for Windows
          </a>
          <Link
            href="/docs"
            className="inline-flex items-center gap-2 rounded-full border border-white/15 px-5 py-2.5 text-sm font-medium text-white transition hover:border-white/40"
          >
            Read the docs <ArrowRight className="size-4" />
          </Link>
        </Reveal>
      </div>

      <div className="relative mx-auto mt-20 max-w-6xl px-4 md:px-6">
        <div className="absolute inset-x-10 top-10 -z-0 h-2/3 rounded-full bg-white/10 blur-[120px]" />
        <div className="tilt relative overflow-hidden rounded-2xl border border-white/10 bg-[#0a0a0a] shadow-[0_40px_120px_-20px_rgba(0,0,0,0.9)]">
          <div className="flex h-9 items-center justify-between border-b border-white/[0.06] px-4">
            <span className="text-xs text-white/40">Granary</span>
            <span className="flex gap-4 text-white/30">
              <span className="h-px w-2.5 self-center bg-current" />
              <span className="size-2.5 border border-current" />
              <span className="text-xs leading-none">✕</span>
            </span>
          </div>
          <img src={asset("/screens/detail.webp")} alt="Granary's detail page" className="block w-full" />
        </div>
      </div>
    </section>
  );
}

function Marquee() {
  return (
    <div className="marquee overflow-hidden border-y border-white/[0.06] py-5">
      <div className="marquee-track flex w-max gap-12 font-mono text-sm whitespace-nowrap text-white/35">
        {[...STACK, ...STACK].map((s, i) => (
          <span key={i} className="flex items-center gap-12">
            {s}
            <span className="text-white/15">/</span>
          </span>
        ))}
      </div>
    </div>
  );
}

function Card({
  icon,
  title,
  children,
  demo,
  className = "",
  delay = 0,
}: {
  icon: ReactNode;
  title: string;
  children: ReactNode;
  demo?: ReactNode;
  className?: string;
  delay?: number;
}) {
  return (
    <Reveal delay={delay} className={className}>
      <Spotlight className="flex h-full flex-col overflow-hidden rounded-2xl border border-white/[0.08] bg-white/[0.015] transition-colors hover:border-white/20">
        {demo ? <div className="relative h-44 overflow-hidden border-b border-white/[0.06]">{demo}</div> : null}
        <div className="p-6">
          <div className="flex items-center gap-2 text-white">
            <span className="text-white/60">{icon}</span>
            <h3 className="font-medium">{title}</h3>
          </div>
          <p className="mt-2 text-sm leading-relaxed text-white/50">{children}</p>
        </div>
      </Spotlight>
    </Reveal>
  );
}

function HdrDemo() {
  return (
    <div className="flex h-full items-end justify-center gap-2 px-10 pb-6">
      {[0.2, 0.5, 0.9, 1.3, 0.7, 1.1, 0.4, 0.8].map((d, i) => (
        <div key={i} className="relative h-full w-5 overflow-hidden rounded-sm bg-white/[0.04]">
          <div
            className="anim-nits absolute inset-x-0 bottom-0 rounded-sm bg-gradient-to-t from-white/20 to-white"
            style={{ animationDelay: `${-d * 2}s` }}
          />
        </div>
      ))}
      <span className="absolute top-4 right-5 font-mono text-[11px] text-white/40">peak 1015 nits · DXGI</span>
    </div>
  );
}

function LaunchDemo() {
  return (
    <div className="relative h-full bg-black">
      <div className="anim-drift absolute inset-0 bg-[radial-gradient(ellipse_at_70%_30%,#3a3a3a,transparent_60%)]" />
      <div className="absolute inset-x-0 bottom-0 p-6">
        <p className="text-xl font-semibold tracking-tight text-white">Northern Lights</p>
        <div className="relative mt-3 h-[2px] w-40 overflow-hidden rounded-full bg-white/15">
          <span className="anim-scan absolute inset-y-0 left-0 w-1/3 rounded-full bg-white" />
        </div>
        <p className="mt-2 font-mono text-[10px] tracking-[0.25em] text-white/40 uppercase">Preparing playback</p>
      </div>
    </div>
  );
}

function ScrubDemo() {
  return (
    <div className="relative h-full">
      <div className="absolute inset-x-8 bottom-10 h-1.5 rounded-full bg-white/15">
        <div className="h-full w-1/3 rounded-full bg-white/80" />
      </div>
      <div className="anim-scrub absolute bottom-14 flex -translate-x-1/2 flex-col items-center gap-1.5">
        <div className="h-16 w-28 rounded-md border border-white/20 bg-[linear-gradient(90deg,#d4d4d4_0_16%,#a3a3a3_16%_33%,#737373_33%_50%,#525252_50%_66%,#404040_66%_83%,#262626_83%)]" />
        <span className="rounded bg-white/10 px-1.5 py-0.5 font-mono text-[10px] text-white/70">Finale · 1:24:07</span>
      </div>
    </div>
  );
}

function TracksDemo() {
  const rows = [
    ["English", "TrueHD Atmos · 7.1", true],
    ["Deutsch", "DTS-HD MA · 5.1", false],
    ["日本語", "AC-3 · 2.0", false],
  ] as const;
  return (
    <div className="flex h-full flex-col justify-center gap-1.5 px-8">
      {rows.map(([lang, info, on]) => (
        <div
          key={lang}
          className={`flex items-center justify-between rounded-lg px-3 py-2 text-sm ${
            on ? "bg-white/10 text-white" : "text-white/45"
          }`}
        >
          <span className="font-medium">{lang}</span>
          <span className="font-mono text-[11px] text-white/45">{info}</span>
        </div>
      ))}
    </div>
  );
}

function Features() {
  return (
    <section id="features" className="mx-auto max-w-6xl scroll-mt-20 px-4 py-32 md:px-6">
      <Reveal>
        <p className="font-mono text-xs tracking-[0.25em] text-white/40 uppercase">Features</p>
        <h2 className="mt-4 max-w-3xl text-4xl font-semibold tracking-tight text-white md:text-5xl">
          Minimal on the outside. <span className="text-white/45">Obsessive underneath.</span>
        </h2>
      </Reveal>
      <div className="mt-14 grid gap-4 md:grid-cols-6">
        <Card
          className="md:col-span-4"
          icon={<MonitorCheck className="size-4" />}
          title="HDR that respects your panel"
          demo={<HdrDemo />}
        >
          Tone mapping targets the real peak brightness Windows reports for the monitor the player is on, with per-monitor
          overrides. Windows HDR always on? Fine: video goes out in HDR without any switching.
        </Card>
        <Card
          className="md:col-span-2"
          icon={<ShieldCheck className="size-4" />}
          title="Always restored"
          delay={80}
        >
          HDR and refresh-rate changes are written to a restore marker before they happen and repaired on the next start,
          even after a crash.
        </Card>
        <Card className="md:col-span-2" icon={<Sparkles className="size-4" />} title="A launch screen, not a void" demo={<LaunchDemo />}>
          From Play to the first frame you see the title, its artwork and a quiet loading line.
        </Card>
        <Card
          className="md:col-span-2"
          icon={<Rewind className="size-4" />}
          title="Seek previews everywhere"
          demo={<ScrubDemo />}
          delay={80}
        >
          Server trickplay when it exists, frames decoded from the file itself when it does not. HDR is tone-mapped.
        </Card>
        <Card
          className="md:col-span-2"
          icon={<Languages className="size-4" />}
          title="Language first"
          demo={<TracksDemo />}
          delay={160}
        >
          Audio and subtitle pickers lead with the language, then codec, channels, Atmos, forced and SDH.
        </Card>
        <Card className="md:col-span-3" icon={<KeyRound className="size-4" />} title="Tokens never reach the UI">
          Sign-ins live in Windows Credential Manager. The Rust host makes every request; the WebView and mpv only ever
          see loopback URLs.
        </Card>
        <Card className="md:col-span-3" icon={<AudioLines className="size-4" />} title="Receiver ready, opt-in" delay={80}>
          Passthrough for AC-3, E-AC-3, TrueHD, DTS and DTS-HD, exclusive WASAPI, channel layouts and normalised
          downmix. Off by default: the primary setup is a desktop.
        </Card>
      </div>
    </section>
  );
}

function Screens() {
  const shots = [
    ["library", "Library with A–Z rail, filters and quality tags"],
    ["detail-audio", "Language-first audio picker"],
    ["person", "Person pages and filmographies"],
    ["settings", "Per-monitor display settings"],
  ] as const;
  return (
    <section className="py-24">
      <Reveal className="mx-auto max-w-6xl px-4 md:px-6">
        <p className="font-mono text-xs tracking-[0.25em] text-white/40 uppercase">Tour</p>
        <h2 className="mt-4 text-4xl font-semibold tracking-tight text-white md:text-5xl">The whole library, calm.</h2>
      </Reveal>
      <div className="mt-12 flex snap-x snap-mandatory gap-5 overflow-x-auto px-4 pb-6 md:px-[max(1.5rem,calc((100vw-72rem)/2+1.5rem))]">
        {shots.map(([file, caption], i) => (
          <Reveal key={file} delay={i * 80} className="w-[85vw] max-w-[720px] shrink-0 snap-center">
            <figure>
              <div className="overflow-hidden rounded-xl border border-white/10">
                <img src={asset(`/screens/${file}.webp`)} alt={caption} className="block w-full" loading="lazy" />
              </div>
              <figcaption className="mt-3 text-sm text-white/45">{caption}</figcaption>
            </figure>
          </Reveal>
        ))}
      </div>
    </section>
  );
}

function Stats() {
  const stats = [
    ["0", "tokens in the WebView or mpv"],
    ["12", "checks before every commit"],
    ["46", "server operations, each cited upstream"],
    ["1", "restore marker behind every display change"],
  ] as const;
  return (
    <section className="border-y border-white/[0.06]">
      <div className="mx-auto grid max-w-6xl grid-cols-2 md:grid-cols-4">
        {stats.map(([n, label], i) => (
          <Reveal
            key={label}
            delay={i * 80}
            className={`px-6 py-12 ${i > 0 ? "md:border-l" : ""} ${i % 2 === 1 ? "border-l" : ""} border-white/[0.06]`}
          >
            <p className="text-4xl font-semibold tracking-tight text-white">{n}</p>
            <p className="mt-2 text-sm text-white/45">{label}</p>
          </Reveal>
        ))}
      </div>
    </section>
  );
}

function BuiltWith() {
  return (
    <section className="mx-auto max-w-6xl px-4 py-32 md:px-6">
      <Reveal>
        <Spotlight className="overflow-hidden rounded-3xl border border-white/10 bg-gradient-to-b from-white/[0.04] to-transparent p-8 md:p-14">
          <p className="font-mono text-xs tracking-[0.25em] text-white/40 uppercase">Built in the open</p>
          <h2 className="mt-4 max-w-2xl text-3xl font-semibold tracking-tight text-white md:text-4xl">
            Written with Claude Code, checked like production software.
          </h2>
          <p className="mt-5 max-w-2xl leading-relaxed text-white/55">
            Granary was built by an AI coding agent working from a written specification, with every step gated on a
            green local check suite and every decision recorded. The working files stay in the repository: the agent
            instructions, the progress log, the architecture decisions and the reviews.
          </p>
          <div className="mt-8 flex flex-wrap gap-2 font-mono text-xs">
            {[
              ["CLAUDE.md", "CLAUDE.md"],
              ["PROGRESS.md", "PROGRESS.md"],
              ["docs/adr/", "docs/adr"],
              ["docs/reviews/", "docs/reviews"],
              ["silo-api.md", "docs/research/silo-api.md"],
            ].map(([label, path]) => (
              <a
                key={path}
                href={blob(path)}
                className="rounded-full border border-white/10 px-3 py-1.5 text-white/60 transition hover:border-white/30 hover:text-white"
              >
                {label}
              </a>
            ))}
          </div>
        </Spotlight>
      </Reveal>
    </section>
  );
}

function Cta() {
  return (
    <section className="relative overflow-hidden px-4 pb-32 text-center">
      <div className="pointer-events-none absolute inset-x-0 bottom-0 h-80 bg-[radial-gradient(ellipse_at_bottom,rgba(255,255,255,0.12),transparent_70%)]" />
      <Reveal>
        <img src={asset("/icon.svg")} alt="" width={72} height={72} className="mx-auto" />
        <h2 className="text-shine mx-auto mt-8 max-w-2xl text-4xl font-semibold tracking-tight md:text-6xl">
          Press play.
        </h2>
        <p className="mx-auto mt-4 max-w-md text-white/50">
          Free and open source under the AGPL. Windows 11 x64, per-user installer, no admin rights.
        </p>
        <div className="mt-10 flex flex-wrap justify-center gap-3">
          <a
            href={releases}
            className="inline-flex items-center gap-2 rounded-full bg-white px-5 py-2.5 text-sm font-medium text-black transition hover:bg-white/85"
          >
            <Download className="size-4" /> Download
          </a>
          <Link
            href="/docs/installation"
            className="inline-flex items-center gap-2 rounded-full border border-white/15 px-5 py-2.5 text-sm font-medium text-white transition hover:border-white/40"
          >
            Installation guide
          </Link>
        </div>
      </Reveal>
    </section>
  );
}

function Footer() {
  return (
    <footer className="border-t border-white/[0.06]">
      <div className="mx-auto flex max-w-6xl flex-col gap-4 px-4 py-10 text-xs text-white/35 md:flex-row md:items-center md:justify-between md:px-6">
        <p>
          Granary is an independent project, compatible with Silo media servers. It is not affiliated with or endorsed
          by Silo Media L.L.C.; “Silo” is their trademark.
        </p>
        <div className="flex gap-5">
          <Link href="/docs" className="hover:text-white">
            Docs
          </Link>
          <a href={repo} className="hover:text-white">
            GitHub
          </a>
          <a href={blob("LICENSE")} className="hover:text-white">
            AGPL-3.0
          </a>
        </div>
      </div>
    </footer>
  );
}

export default function Home() {
  return (
    <main className="landing min-h-screen">
      <Nav />
      <Hero />
      <Marquee />
      <Features />
      <section className="mx-auto max-w-6xl px-4 py-16 md:px-6">
        <Pipeline />
      </section>
      <Screens />
      <Stats />
      <BuiltWith />
      <Cta />
      <Footer />
    </main>
  );
}
