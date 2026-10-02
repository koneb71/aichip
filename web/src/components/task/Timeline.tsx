import { useEffect, useState } from "react";
import { CheckSquare, MessageSquare, Play, ScrollText } from "lucide-react";
import { api, TimelineEvent } from "../../lib/api";
import { cn } from "../ui/cn";

/**
 * One card's whole story, oldest first: what was said, every run and how it
 * ended, every check, and what was done to it. "What happened here
 * overnight" as one read.
 */
export function Timeline({ taskId, refreshKey }: { taskId: string; refreshKey: string }) {
  const [events, setEvents] = useState<TimelineEvent[] | null>(null);

  useEffect(() => {
    let stale = false;
    api
      .timeline(taskId)
      .then((r) => !stale && setEvents(r.events))
      .catch(() => !stale && setEvents([]));
    return () => {
      stale = true;
    };
  }, [taskId, refreshKey]);

  if (events === null) return <div className="skeleton h-40 rounded-lg" />;
  if (events.length === 0) return <p className="text-xs text-fg-muted">Nothing has happened on this card yet.</p>;

  return (
    <ol className="relative ml-2 border-l border-border">
      {events.map((e, i) => (
        <li key={i} className="relative pb-4 pl-5 last:pb-0">
          <span
            className={cn(
              "absolute -left-[9px] top-0.5 grid size-[17px] place-items-center rounded-full border border-border bg-panel",
              e.kind === "run" && e.status === "failed" && "border-[color-mix(in_oklab,var(--color-danger)_40%,var(--color-border))] text-danger-fg",
            )}
          >
            <Icon kind={e.kind} />
          </span>
          <div className="flex flex-wrap items-baseline gap-x-2 text-xs">
            {e.actor && <span className="font-medium text-fg">{e.actor}</span>}
            <span className="text-fg-muted">{e.title}</span>
            {e.costUsd != null && <span className="tabular font-mono text-[11px] text-fg-subtle">${e.costUsd.toFixed(3)}</span>}
            <span className="tabular ml-auto text-[11px] text-fg-subtle">{new Date(e.at).toLocaleString()}</span>
          </div>
          {e.detail && <p className="mt-1 line-clamp-3 whitespace-pre-wrap break-words text-xs leading-relaxed text-fg-muted">{e.detail}</p>}
        </li>
      ))}
    </ol>
  );
}

function Icon({ kind }: { kind: TimelineEvent["kind"] }) {
  const cls = "size-2.5";
  switch (kind) {
    case "comment":
      return <MessageSquare className={cls} />;
    case "run":
      return <Play className={cls} />;
    case "checks":
      return <CheckSquare className={cls} />;
    default:
      return <ScrollText className={cls} />;
  }
}
