import { useEffect, useRef, useState } from "react";

export interface StreamEvent {
  runId: string;
  seq: number;
  ts: string;
  type: string;
  [key: string]: unknown;
}

/** Subscribe to a run's event stream with DB replay + live tail. */
export function useRunStream(runId: string | null) {
  const [events, setEvents] = useState<StreamEvent[]>([]);
  const seenSeq = useRef(new Set<string>());

  useEffect(() => {
    setEvents([]);
    seenSeq.current.clear();
    if (!runId) return;

    const proto = location.protocol === "https:" ? "wss" : "ws";
    const socket = new WebSocket(
      `${proto}://${location.host}/ws?run_id=${runId}&after_seq=-1`,
    );
    socket.onmessage = (msg) => {
      try {
        const raw = JSON.parse(msg.data);
        // Replay frames nest the payload under `event`; live frames are flat.
        // `step_id` sits on the envelope in both cases and must be lifted out
        // explicitly — spreading only `raw.event` drops it, which silently
        // costs every multi-agent view its ability to say *who* acted.
        const event: StreamEvent = raw.event
          ? {
              runId: raw.runId ?? raw.run_id,
              seq: raw.seq,
              ts: raw.ts,
              step_id: raw.step_id ?? raw.stepId,
              ...raw.event,
            }
          : { runId: raw.run_id, seq: raw.seq, ts: raw.ts, ...raw };
        const key = `${event.seq}:${event.type}`;
        if (event.seq >= 0 && seenSeq.current.has(key)) return;
        seenSeq.current.add(key);
        setEvents((prev) => [...prev, event]);
      } catch {
        // ignore malformed frames
      }
    };
    return () => socket.close();
  }, [runId]);

  return events;
}
