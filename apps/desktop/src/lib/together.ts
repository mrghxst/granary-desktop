import { invoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/**
 * Watch Together (silo-api.md §4.1i): types of the room snapshot and socket frames
 * (`silo-server/web/src/lib/watchTogether.ts@749db96a`, `internal/watchtogether/types.go@749db96a`) and the pure parts of
 * the sync, after `web/src/player/hooks/useWatchTogetherPlaybackSync.ts@749db96a`.
 */

export type RoomPhase = "lobby" | "playing" | "ended";
export type RoomPlaybackState = "idle" | "waiting" | "paused" | "playing";

export interface RoomMember {
  profile_id: string;
  display_name: string;
  is_host: boolean;
  is_self: boolean;
  connected: boolean;
  is_ready?: boolean;
  is_buffering?: boolean;
}

export interface RoomSnapshot {
  room_id: string;
  phase: RoomPhase;
  playback_state: RoomPlaybackState;
  selection_mode: string;
  selection_revision: number;
  /** HTTP sends ids as strings, socket frames as numbers. */
  selected_content_id?: string | number;
  selected_file_id?: string | number;
  code: string;
  guest_control_policy: string;
  is_paused: boolean;
  anchor_position_seconds: number;
  anchor_updated_at: string;
  generation: number;
  member_count: number;
  host_connected: boolean;
  self_role: "host" | "guest";
  self_can_control_transport: boolean;
  self_can_manage_room: boolean;
  self_ignore_wait: boolean;
  attached_session_id?: string;
  members?: RoomMember[];
}

export interface TransportCommand {
  command_id: string;
  session_id?: string;
  selection_revision: number;
  action: "play" | "pause" | "seek";
  position_seconds: number;
  execute_at: string;
  issued_at: string;
  playback_state: RoomPlaybackState;
}

export interface TogetherCapabilities {
  state: string;
  allowed: boolean;
  socket_protocol: string;
}

export type LinkState = "connecting" | "connected" | "closed";

export type TogetherEvent =
  | { kind: "frame"; frame: { type: string } & Record<string, unknown> }
  | { kind: "status"; state: LinkState; reason?: string | null };

const noHost = () => Promise.reject(Object.assign(new Error("not running inside Granary"), { code: "no_host" }));

export const together = {
  capabilities: (serverId: string) =>
    isTauri() ? invoke<TogetherCapabilities>("together_capabilities", { serverId }) : noHost(),
  start: (serverId: string, contentId: string, fileId: string | null) =>
    isTauri() ? invoke<{ room: RoomSnapshot }>("together_start", { serverId, contentId, fileId }) : noHost(),
  join: (serverId: string, code: string) =>
    isTauri() ? invoke<{ room: RoomSnapshot }>("together_join", { serverId, code }) : noHost(),
  send: (frame: { type: string } & Record<string, unknown>) =>
    isTauri() ? invoke<null>("together_send", { frame }) : noHost(),
  leave: () => (isTauri() ? invoke<null>("together_leave") : noHost()),
  onEvent: (cb: (e: TogetherEvent) => void) =>
    isTauri()
      ? listen<TogetherEvent>("together://event", (e) => {
          cb(e.payload);
        })
      : Promise.resolve(() => undefined),
};

/** An id from a snapshot as the catalog uses it. */
export function roomId(v: string | number | undefined | null): string | null {
  return v === undefined || v === null || v === "" ? null : String(v);
}

/** One ping/pong exchange: the server clock minus ours (ms) and the round trip. */
export interface ClockSample {
  offset: number;
  rtt: number;
}

export function clockSample(
  clientSent: number,
  serverReceived: number,
  serverSent: number,
  clientReceived: number,
): ClockSample | null {
  if (![clientSent, serverReceived, serverSent, clientReceived].every(Number.isFinite)) return null;
  const rtt = clientReceived - clientSent - (serverSent - serverReceived);
  if (rtt < 0) return null;
  return { offset: (serverReceived - clientSent + (serverSent - clientReceived)) / 2, rtt };
}

/** The offset of the quickest of the recent samples (the least queueing); null before the first. */
export function clockOffset(samples: readonly ClockSample[]): number | null {
  let best: ClockSample | null = null;
  for (const s of samples) if (!best || s.rtt < best.rtt) best = s;
  return best ? best.offset : null;
}

/** Keep the latest few samples. */
export function addSample(samples: readonly ClockSample[], s: ClockSample, keep = 8): ClockSample[] {
  return [...samples, s].slice(-keep);
}

/** Where the room is for a command at server time `nowServer` (ms): a playing room moves on at 1x past `execute_at`. */
export function commandPosition(cmd: TransportCommand, nowServer: number): number {
  const at = Date.parse(cmd.execute_at);
  if (cmd.playback_state !== "playing" || !Number.isFinite(at) || nowServer <= at) return cmd.position_seconds;
  return cmd.position_seconds + (nowServer - at) / 1000;
}

/** Milliseconds until a command is due, on our clock. */
export function commandDelay(cmd: TransportCommand, nowLocal: number, offset: number): number {
  const at = Date.parse(cmd.execute_at);
  return Number.isFinite(at) ? Math.max(0, at - offset - nowLocal) : 0;
}

/** Where the room is from a snapshot (its anchor, moving on at 1x while playing). */
export function anchorPosition(room: RoomSnapshot, nowServer: number): number {
  const at = Date.parse(room.anchor_updated_at);
  if (room.playback_state !== "playing" || room.is_paused || !Number.isFinite(at) || nowServer <= at) {
    return room.anchor_position_seconds;
  }
  return room.anchor_position_seconds + (nowServer - at) / 1000;
}

/** How far from a seek's destination an acknowledgement may land (`realtime-api.md`, "Room playback readiness"). */
export function seekTolerance(host: boolean, ahead: boolean): number {
  if (host) return 15;
  return ahead ? 1 : 5;
}

export interface ReadyInput {
  room: RoomSnapshot;
  command: TransportCommand | null;
  appliedCommandId: string | null;
  sessionId: string | null;
  attached: boolean;
  /** The player is seeking, loading or buffering. */
  settling: boolean;
  position: number;
}

/** Whether this viewer may acknowledge the room's latest command now; the reason when not. */
export function readyCheck(i: ReadyInput): { ok: true; commandId: string } | { ok: false; reason: string } {
  const catchingUp = i.room.self_ignore_wait;
  if (!i.sessionId || !i.attached) return { ok: false, reason: "session not attached" };
  if (i.room.playback_state !== "waiting" && !catchingUp) return { ok: false, reason: "room not waiting" };
  const c = i.command;
  if (!c || (c.playback_state !== i.room.playback_state && !catchingUp)) return { ok: false, reason: "no command" };
  if (c.selection_revision !== i.room.selection_revision) return { ok: false, reason: "old selection" };
  if (c.session_id && c.session_id !== i.sessionId) return { ok: false, reason: "another session" };
  if (i.appliedCommandId !== c.command_id) return { ok: false, reason: "not applied yet" };
  if (i.settling) return { ok: false, reason: "settling" };
  if (i.room.playback_state === "waiting" && c.action === "seek") {
    const off = i.position - c.position_seconds;
    if (Math.abs(off) > seekTolerance(i.room.self_role === "host", off > 0)) {
      return { ok: false, reason: "away from the seek target" };
    }
  }
  return { ok: true, commandId: c.command_id };
}

/** This viewer's member entry says it is ready (older servers list no members: assume not). */
export function selfReady(room: RoomSnapshot): boolean {
  return room.members?.some((m) => m.is_self && m.is_ready === true) ?? false;
}

let partyActive = false;
/** A Watch Party is joined: the room, not autoplay, decides what plays next. */
export function inWatchParty(): boolean {
  return partyActive;
}
export function setWatchPartyActive(on: boolean): void {
  partyActive = on;
}
