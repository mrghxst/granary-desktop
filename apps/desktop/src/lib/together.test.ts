import { describe, expect, it } from "vitest";
import {
  addSample,
  anchorPosition,
  clockOffset,
  clockSample,
  commandDelay,
  commandPosition,
  readyCheck,
  roomId,
  type RoomSnapshot,
  type TransportCommand,
} from "./together";

const room: RoomSnapshot = {
  room_id: "r1",
  phase: "playing",
  playback_state: "waiting",
  selection_mode: "host_pick",
  selection_revision: 2,
  selected_content_id: 42,
  code: "ABC123",
  guest_control_policy: "host_only",
  is_paused: true,
  anchor_position_seconds: 100,
  anchor_updated_at: "2026-11-17T12:00:00.000Z",
  generation: 5,
  member_count: 2,
  host_connected: true,
  self_role: "guest",
  self_can_control_transport: false,
  self_can_manage_room: false,
  self_ignore_wait: false,
};

const cmd: TransportCommand = {
  command_id: "c1",
  selection_revision: 2,
  action: "seek",
  position_seconds: 100,
  execute_at: "2026-11-17T12:00:01.000Z",
  issued_at: "2026-11-17T12:00:00.500Z",
  playback_state: "waiting",
};
const T = Date.parse("2026-11-17T12:00:01.000Z");

describe("room clock", () => {
  it("estimates the offset from the quickest exchange", () => {
    // Server is 5 s ahead; 40 ms each way, then a slow 400 ms exchange.
    const a = clockSample(1000, 6040, 6041, 1081);
    const b = clockSample(2000, 7300, 7301, 2401);
    expect(a?.rtt).toBe(80);
    expect(a?.offset).toBeCloseTo(5000, 0);
    if (!a || !b) throw new Error("no samples");
    const samples = addSample(addSample([], b), a);
    expect(clockOffset(samples)).toBeCloseTo(5000, 0);
    expect(clockOffset([])).toBeNull();
    expect(clockSample(10, 5, 6, 8)).toBeNull();
  });
});

describe("commands", () => {
  it("projects a late play command forward and leaves waiting ones in place", () => {
    const play = { ...cmd, action: "play" as const, playback_state: "playing" as const };
    expect(commandPosition(play, T + 2500)).toBeCloseTo(102.5);
    expect(commandPosition(play, T - 100)).toBe(100);
    expect(commandPosition(cmd, T + 2500)).toBe(100);
    // Our clock 5 s behind the server's: due 1 s from now on our clock.
    expect(commandDelay(cmd, T - 5000 - 1000, 5000)).toBe(1000);
    expect(commandDelay(cmd, T, 0)).toBe(0);
  });

  it("projects a playing anchor", () => {
    const playing = { ...room, playback_state: "playing" as const, is_paused: false };
    expect(anchorPosition(playing, Date.parse(room.anchor_updated_at) + 3000)).toBeCloseTo(103);
    expect(anchorPosition(room, Date.parse(room.anchor_updated_at) + 3000)).toBe(100);
  });

  it("reads ids from both shapes", () => {
    expect(roomId(42)).toBe("42");
    expect(roomId("7")).toBe("7");
    expect(roomId(undefined)).toBeNull();
  });
});

describe("readyCheck", () => {
  const base = {
    room,
    command: cmd,
    appliedCommandId: "c1",
    sessionId: "ps-1",
    attached: true,
    settling: false,
    position: 100.4,
  };

  it("acknowledges an applied command once the player settled at the target", () => {
    expect(readyCheck(base)).toEqual({ ok: true, commandId: "c1" });
  });

  it("waits for attachment, application, settling and the seek target", () => {
    expect(readyCheck({ ...base, attached: false }).ok).toBe(false);
    expect(readyCheck({ ...base, appliedCommandId: null }).ok).toBe(false);
    expect(readyCheck({ ...base, settling: true }).ok).toBe(false);
    expect(readyCheck({ ...base, position: 103 }).ok).toBe(false);
    // A guest may land a little short; the host anywhere within 15 s.
    expect(readyCheck({ ...base, position: 97 }).ok).toBe(true);
    expect(readyCheck({ ...base, room: { ...room, self_role: "host" }, position: 110 }).ok).toBe(true);
    expect(readyCheck({ ...base, command: { ...cmd, selection_revision: 1 } }).ok).toBe(false);
    expect(readyCheck({ ...base, room: { ...room, playback_state: "playing" } }).ok).toBe(false);
  });
});
