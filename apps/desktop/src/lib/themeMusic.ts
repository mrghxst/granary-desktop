/**
 * A title's theme tune on its detail page, as in the web app (`silo-server/web/src/lib/themeMusic.ts@749db96a`):
 * quiet (35 %), faded in and out, stopped while something plays. The audio is decoded with Web Audio from bytes
 * the host fetched, so the page needs no media element and the content security policy stays `media-src 'none'`.
 */

const VOLUME = 0.35;
const FADE_SECONDS = 0.6;

export type ThemeLoader = (ownerId: string, themeId: string) => Promise<ArrayBuffer>;

interface Voice {
  source: AudioBufferSourceNode;
  gain: GainNode;
}

export class ThemeMusic {
  private context: AudioContext | null = null;
  private voice: Voice | null = null;
  private key = "";
  /** Bumped on every change of tune: a load that finishes late belongs to an older one and is dropped. */
  private generation = 0;

  constructor(
    private readonly load: ThemeLoader,
    private readonly createContext: () => AudioContext = () => new AudioContext(),
  ) {}

  /** Play `themeId` of `ownerId`; the tune that is already playing carries on. */
  play(ownerId: string, themeId: string, loop: boolean): void {
    const key = `${ownerId}/${themeId}`;
    if (key === this.key) {
      if (this.voice) this.voice.source.loop = loop;
      return;
    }
    this.stop();
    this.key = key;
    const generation = this.generation;
    void this.start(ownerId, themeId, loop, generation);
  }

  private async start(ownerId: string, themeId: string, loop: boolean, generation: number): Promise<void> {
    try {
      const bytes = await this.load(ownerId, themeId);
      if (generation !== this.generation) return;
      this.context ??= this.createContext();
      const context = this.context;
      const buffer = await context.decodeAudioData(bytes);
      if (generation !== this.generation) return;
      if (context.state === "suspended") await context.resume();
      if (generation !== this.generation) return;
      const source = context.createBufferSource();
      const gain = context.createGain();
      source.buffer = buffer;
      source.loop = loop;
      gain.gain.setValueAtTime(0, context.currentTime);
      gain.gain.linearRampToValueAtTime(VOLUME, context.currentTime + FADE_SECONDS);
      source.connect(gain).connect(context.destination);
      source.start();
      this.voice = { source, gain };
    } catch {
      // Themes are optional: a refused grant or audio that does not decode never disturbs the page.
    }
  }

  /** Fade out and stop; `now` cuts at once (playback is starting). */
  stop(now = false): void {
    this.generation += 1;
    this.key = "";
    const voice = this.voice;
    this.voice = null;
    if (!voice || !this.context) return;
    const { source, gain } = voice;
    try {
      if (now) {
        source.stop();
      } else {
        const t = this.context.currentTime;
        gain.gain.cancelScheduledValues(t);
        gain.gain.setValueAtTime(gain.gain.value, t);
        gain.gain.linearRampToValueAtTime(0, t + FADE_SECONDS);
        source.stop(t + FADE_SECONDS);
      }
    } catch {
      // Already stopped.
    }
  }

  /** Stop and release the audio device. */
  close(): void {
    this.stop(true);
    void this.context?.close().catch(() => undefined);
    this.context = null;
  }
}
