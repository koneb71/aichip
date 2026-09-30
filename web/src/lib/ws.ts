import { useEffect, useState } from "react";

export interface StreamEvent {
  runId: string;
  seq: number;
  ts: string;
  type: string;
  [key: string]: unknown;
}

/** One frame off the socket, in the shape the views read. */
export function parseFrame(data: string): StreamEvent | null {
  try {
    const raw = JSON.parse(data);
    // Replay frames nest the payload under `event`; live frames are flat.
    // `step_id` sits on the envelope in both cases and must be lifted out
    // explicitly — spreading only `raw.event` drops it, which silently
    // costs every multi-agent view its ability to say *who* acted.
    return raw.event
      ? {
          runId: raw.runId ?? raw.run_id,
          seq: raw.seq,
          ts: raw.ts,
          step_id: raw.step_id ?? raw.stepId,
          ...raw.event,
        }
      : { runId: raw.run_id, seq: raw.seq, ts: raw.ts, ...raw };
  } catch {
    return null; // a malformed frame is dropped, not fatal
  }
}

/**
 * Which logged events have arrived, as a resume point plus stragglers.
 *
 * Seqs are allocated from one counter per run but published by concurrent
 * steps, so they can arrive out of order — 11 before 10. The highest seq seen
 * is therefore *not* a safe place to resume from; `floor` is: every seq at or
 * below it has arrived. Only arrivals above the floor are remembered, so the
 * set holds stragglers rather than the whole transcript.
 */
export class SeqLedger {
  floor = -1;
  private above = new Set<number>();

  /** True the first time a seq is seen; false for a duplicate. */
  admit(seq: number): boolean {
    if (seq < 0) return true; // ephemeral (permission) events are never logged
    if (seq <= this.floor || this.above.has(seq)) return false;
    this.above.add(seq);
    while (this.above.delete(this.floor + 1)) this.floor++;
    return true;
  }
}

/** How long to wait before reconnect attempt `n` (0-based). */
export function backoff(n: number): number {
  return Math.min(10_000, 500 * 2 ** n);
}

/**
 * Subscribe to a run's event stream with DB replay + live tail.
 *
 * The socket is expected to drop — a server restart, a laptop lid, a proxy
 * timeout — and a view that froze silently on the last frame it got was the
 * old behaviour. It reconnects with backoff and resumes from the ledger's
 * floor, and the ledger discards whatever the replay sends twice.
 */
export function useRunStream(runId: string | null) {
  const [events, setEvents] = useState<StreamEvent[]>([]);

  useEffect(() => {
    setEvents([]);
    if (!runId) return;

    const ledger = new SeqLedger();
    let socket: WebSocket | null = null;
    let attempt = 0;
    let retry: ReturnType<typeof setTimeout> | undefined;
    let stopped = false;

    // One state update per short batch, not per message: appending with
    // `[...prev, e]` on every message copies the whole transcript each time,
    // which is quadratic over a long run. A timer rather than an animation
    // frame, because frames stop in a background tab and a view watching for
    // "run completed" should not have to wait for someone to look at it.
    let pending: StreamEvent[] = [];
    let batch: ReturnType<typeof setTimeout> | undefined;
    const flush = () => {
      batch = undefined;
      const arrived = pending;
      pending = [];
      setEvents((prev) => prev.concat(arrived));
    };

    const connect = () => {
      const proto = location.protocol === "https:" ? "wss" : "ws";
      socket = new WebSocket(
        `${proto}://${location.host}/ws?run_id=${runId}&after_seq=${ledger.floor}`,
      );
      socket.onopen = () => {
        attempt = 0;
      };
      socket.onmessage = (msg) => {
        const event = parseFrame(msg.data);
        if (!event || !ledger.admit(event.seq)) return;
        pending.push(event);
        batch ??= setTimeout(flush, 32);
      };
      socket.onclose = () => {
        if (stopped) return;
        retry = setTimeout(connect, backoff(attempt++));
      };
    };
    connect();

    return () => {
      stopped = true;
      clearTimeout(retry);
      clearTimeout(batch);
      socket?.close();
    };
  }, [runId]);

  return events;
}
