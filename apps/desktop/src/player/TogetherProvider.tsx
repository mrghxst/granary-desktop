import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { player, setTransportGate, type TransportRequest } from "../lib/player";
import {
  addSample,
  anchorPosition,
  clockOffset,
  clockSample,
  commandDelay,
  commandPosition,
  readyCheck,
  roomId,
  selfReady,
  setWatchPartyActive,
  together,
  type ClockSample,
  type LinkState,
  type RoomSnapshot,
  type TransportCommand,
} from "../lib/together";
import { usePlayer } from "./context";
import { TogetherContext, type TogetherApi } from "./togetherContext";

/** State reports while the room plays; readiness is retried faster (`realtime-api.md`, "Room playback readiness"). */
const REPORT_MS = 1500;
const READY_RETRY_MS = 500;
/** Pings come quickly until a few clock samples are in, then rarely. */
const FAST_PINGS = 5;
const PING_FAST_MS = 1000;
const PING_SLOW_MS = 15_000;
/** A play command closer than this to where we already are does not seek. */
const DRIFT_SECONDS = 0.75;

/**
 * Watch Together for the native player, after the web player's room hooks
 * (`silo-server/web/src/player/hooks/useWatchTogether{RoomConnection,PlaybackSync}.ts@749db96a`): the room's selection
 * starts here at the room's position, this playback session is attached to the room, transport commands run at their
 * `execute_at` on the server's clock, readiness is acknowledged once the player settled, and the viewer's own
 * play/pause/seek become `transport_request`s (or are refused when only the host controls playback).
 */
export function TogetherProvider({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const { start: startPlayback, state, plan, ctx, showToast } = usePlayer();
  const [room, setRoom] = useState<RoomSnapshot | null>(null);
  const [serverId, setServerId] = useState<string | null>(null);
  const [link, setLink] = useState<LinkState>("closed");
  const [ended, setEnded] = useState<string | null>(null);
  const [command, setCommand] = useState<TransportCommand | null>(null);
  const [epoch, setEpoch] = useState(0);

  const roomRef = useRef(room);
  const offset = useRef(0);
  const samples = useRef<ClockSample[]>([]);
  const applied = useRef<string | null>(null);
  const startedKey = useRef<string | null>(null);
  const attachedKey = useRef<string | null>(null);
  const live = useRef({ state, plan, ctx, serverId, link });
  useEffect(() => {
    roomRef.current = room;
    live.current = { state, plan, ctx, serverId, link };
    setWatchPartyActive(room !== null);
  });

  const selected = roomId(room?.selected_content_id);
  const sessionId = plan?.session_id ?? null;
  // The playing file is the room's selection (not something the viewer opened on the side).
  const onSelection = room !== null && selected !== null && ctx?.watch.content_id === selected && state.active;

  const reset = useCallback(() => {
    setRoom(null);
    setCommand(null);
    applied.current = null;
    startedKey.current = null;
    attachedKey.current = null;
    samples.current = [];
    offset.current = 0;
  }, []);

  // Socket frames and link status from the host.
  useEffect(() => {
    let alive = true;
    let unlisten: (() => void) | undefined;
    void together
      .onEvent((e) => {
        if (!alive) return;
        if (e.kind === "status") {
          setLink(e.state);
          if (e.state === "connected") setEpoch((n) => n + 1);
          if (e.state === "closed") {
            const reason = e.reason ?? "left";
            setEnded(reason);
            const wasPlaying = roomRef.current !== null && live.current.state.active;
            reset();
            if (reason === "room_closed") {
              showToast(t("together.endedByHost"));
              if (wasPlaying) void player.roomStop().catch(() => undefined);
            } else if (reason === "connection_replaced") {
              showToast(t("together.replaced"));
              if (wasPlaying) void player.roomPause(true).catch(() => undefined);
            } else if (reason !== "left") {
              showToast(t("together.lost"));
            }
          }
          return;
        }
        const f = e.frame;
        if (f.type === "snapshot" && f.room && typeof f.room === "object") {
          const next = f.room as RoomSnapshot;
          setRoom((cur) => (cur && cur.room_id === next.room_id && cur.generation > next.generation ? cur : next));
        } else if (f.type === "transport_command" && f.command && typeof f.command === "object") {
          setCommand(f.command as TransportCommand);
        } else if (f.type === "pong") {
          const s = clockSample(
            Date.parse(String(f.client_sent_at)),
            Date.parse(String(f.server_received_at)),
            Date.parse(String(f.server_sent_at)),
            Date.now(),
          );
          if (s) {
            samples.current = addSample(samples.current, s);
            offset.current = clockOffset(samples.current) ?? offset.current;
          }
        }
      })
      .then((u) => {
        if (alive) unlisten = u;
        else u();
      });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, [reset, showToast, t]);

  // Clock pings while connected.
  useEffect(() => {
    if (link !== "connected") return;
    let sent = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const ping = () => {
      sent += 1;
      void together.send({ type: "ping", client_sent_at: new Date().toISOString() }).catch(() => undefined);
      timer = setTimeout(ping, sent < FAST_PINGS ? PING_FAST_MS : PING_SLOW_MS);
    };
    ping();
    return () => {
      clearTimeout(timer);
    };
  }, [link, epoch]);

  // The room's selection plays here, from where the room is.
  useEffect(() => {
    if (!room || !serverId || room.phase !== "playing" || !selected) return;
    const key = `${room.room_id}:${String(room.selection_revision)}`;
    if (startedKey.current === key) return;
    startedKey.current = key;
    applied.current = null;
    const fileId = roomId(room.selected_file_id);
    void startPlayback(serverId, selected, {
      ...(fileId ? { fileId } : {}),
      startAt: anchorPosition(room, Date.now() + offset.current),
    }).catch(() => {
      showToast(t("together.startFailed"));
    });
  }, [room, serverId, selected, startPlayback, showToast, t]);

  // This playback session belongs to the room (again after every reconnect).
  useEffect(() => {
    if (link !== "connected" || !sessionId || !onSelection) return;
    const key = `${String(epoch)}:${sessionId}`;
    if (attachedKey.current === key) return;
    attachedKey.current = key;
    void together.send({ type: "attach_session", session_id: sessionId }).catch(() => undefined);
  }, [link, epoch, sessionId, onSelection]);

  // Transport commands run at their execute_at on the server's clock.
  useEffect(() => {
    if (!command || !sessionId || !onSelection) return;
    if (applied.current === command.command_id) return;
    if (command.session_id && command.session_id !== sessionId) return;
    const id = setTimeout(
      () => {
        const target = commandPosition(command, Date.now() + offset.current);
        const here = live.current.state.position;
        if (command.action === "seek" || Math.abs(here - target) > DRIFT_SECONDS) {
          void player.roomSeek(target).catch(() => undefined);
        }
        void player.roomPause(command.playback_state !== "playing").catch(() => undefined);
        applied.current = command.command_id;
      },
      commandDelay(command, Date.now(), offset.current),
    );
    return () => {
      clearTimeout(id);
    };
  }, [command, sessionId, onSelection]);

  // State reports, and readiness acknowledgements while the room waits for us.
  const pending = room !== null && (room.playback_state === "waiting" || room.self_ignore_wait) && !selfReady(room);
  useEffect(() => {
    if (link !== "connected" || !sessionId || !onSelection) return;
    const id = setInterval(
      () => {
        const r = roomRef.current;
        const s = live.current.state;
        if (!r) return;
        const settling = s.buffering || s.phase === "loading" || s.phase === "seeking" || s.phase === "buffering";
        const position = Math.max(0, s.position);
        if (pending) {
          const check = readyCheck({
            room: r,
            command,
            appliedCommandId: applied.current,
            sessionId,
            attached: r.attached_session_id === undefined || r.attached_session_id === sessionId,
            settling,
            position,
          });
          if (check.ok) {
            void together
              .send({
                type: r.self_ignore_wait ? "ready" : "state_report",
                session_id: sessionId,
                command_id: check.commandId,
                position_seconds: position,
                is_paused: s.paused,
                is_ready: true,
              })
              .catch(() => undefined);
            return;
          }
        }
        // A stalled player reports where it stopped, not a decision: stay quiet until it moves again.
        if (settling) return;
        void together
          .send({
            type: "state_report",
            session_id: sessionId,
            ...(applied.current ? { command_id: applied.current } : {}),
            position_seconds: position,
            is_paused: s.paused,
          })
          .catch(() => undefined);
      },
      pending ? READY_RETRY_MS : REPORT_MS,
    );
    return () => {
      clearInterval(id);
    };
  }, [link, sessionId, onSelection, pending, command]);

  const leave = useCallback(() => {
    reset();
    void together.leave().catch(() => undefined);
  }, [reset]);

  // The viewer's own play/pause/seek go to the room while the room's selection plays; leaving the player leaves.
  useEffect(() => {
    if (!room || !onSelection) {
      setTransportGate(null, room ? leave : null);
      return;
    }
    const gate = (r: TransportRequest) => {
      const cur = roomRef.current;
      // Without the room link the player acts on its own; the room corrects it once the link is back.
      if (!cur || live.current.link !== "connected") return false;
      if (!cur.self_can_control_transport) {
        showToast(t("together.hostControls"));
        return true;
      }
      const s = live.current.state;
      if ("pause" in r) {
        const paused = r.pause ?? !s.paused;
        void together
          .send({
            type: "transport_request",
            action: paused ? "pause" : "play",
            position_seconds: Math.max(0, s.position),
            is_paused: paused,
          })
          .catch(() => undefined);
      } else {
        const to = Math.max(0, r.relative ? s.position + r.seek : r.seek);
        void together
          .send({ type: "transport_request", action: "seek", position_seconds: to, is_paused: s.paused })
          .catch(() => undefined);
      }
      return true;
    };
    setTransportGate(gate, leave);
    return () => {
      setTransportGate(null, null);
    };
  }, [room, onSelection, leave, showToast, t]);

  const start = useCallback(
    async (server: string, contentId: string, fileId: string | null) => {
      const r = await together.start(server, contentId, fileId);
      reset();
      setEnded(null);
      setServerId(server);
      setRoom(r.room);
    },
    [reset],
  );
  const join = useCallback(
    async (server: string, code: string) => {
      const r = await together.join(server, code);
      reset();
      setEnded(null);
      setServerId(server);
      setRoom(r.room);
    },
    [reset],
  );

  const api = useMemo<TogetherApi>(
    () => ({ room, serverId, link, ended, start, join, leave }),
    [room, serverId, link, ended, start, join, leave],
  );
  return <TogetherContext.Provider value={api}>{children}</TogetherContext.Provider>;
}
