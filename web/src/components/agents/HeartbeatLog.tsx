import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api, Beat } from "../../lib/api";
import { Badge, type Tone } from "../ui/Badge";

const TONE: Record<Beat["outcome"], Tone> = {
  started: "success",
  fired: "accent",
  idle: "neutral",
  busy: "neutral",
  held: "warning",
  paused: "neutral",
};

const SAYS: Record<Beat["outcome"], string> = {
  started: "started",
  fired: "ran its manager pass",
  idle: "nothing to do",
  busy: "already working",
  held: "held",
  paused: "paused",
};

/** What an agent's recent heartbeats did — "is it picking up work, or idling?" */
export function HeartbeatLog({ agentId }: { agentId: string }) {
  const [beats, setBeats] = useState<Beat[] | null>(null);
  useEffect(() => {
    let stale = false;
    api
      .agentHeartbeats(agentId)
      .then((r) => !stale && setBeats(r.beats))
      .catch(() => !stale && setBeats([]));
    return () => {
      stale = true;
    };
  }, [agentId]);
  if (!beats || beats.length === 0) return null;
  return (
    <ul className="mt-2 max-h-48 space-y-1 overflow-y-auto" aria-label="Recent heartbeats">
      {beats.slice(0, 20).map((b, i) => (
        <li key={i} className="flex items-center gap-2 text-[11px]">
          <span className="tabular w-28 shrink-0 text-fg-subtle">{new Date(b.at).toLocaleString([], { dateStyle: "short", timeStyle: "short" })}</span>
          <Badge tone={TONE[b.outcome]}>{SAYS[b.outcome]}</Badge>
          {b.reason === "wake" && <span className="text-fg-subtle">woken</span>}
          {b.taskId && b.projectId ? (
            <Link to={`/projects/${b.projectId}?task=${b.taskId}`} className="min-w-0 truncate text-fg hover:underline">
              {b.taskTitle}
            </Link>
          ) : (
            b.detail && <span className="min-w-0 truncate text-fg-muted" title={b.detail}>{b.detail}</span>
          )}
        </li>
      ))}
    </ul>
  );
}
