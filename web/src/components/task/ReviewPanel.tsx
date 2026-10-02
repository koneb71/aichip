import { useEffect, useState } from "react";
import { CheckCircle2, CircleAlert, RotateCcw } from "lucide-react";
import { api, CardReviews } from "../../lib/api";
import { Badge } from "../ui/Badge";
import { Button } from "../ui/Button";
import { toast } from "../ui/Toast";

/**
 * The card's agent reviews, newest first, with where the loop stands.
 *
 * Shown only where the project asks for one. "Review again" is a person's
 * click, so it gets one round past the cap — the cap bounds what runs
 * unattended, not what you ask for.
 */
export function ReviewPanel({ taskId, busy, refreshKey }: { taskId: string; busy: boolean; refreshKey: string }) {
  const [data, setData] = useState<CardReviews | null>(null);
  const [starting, setStarting] = useState(false);

  useEffect(() => {
    let stale = false;
    api
      .cardReviews(taskId)
      .then((d) => {
        if (!stale) setData(d);
      })
      .catch(() => {});
    return () => {
      stale = true;
    };
  }, [taskId, refreshKey]);

  if (!data || (!data.requireReview && data.reviews.length === 0)) return null;

  const again = async () => {
    setStarting(true);
    try {
      await api.startReview(taskId);
      toast("Review started", { tone: "success" });
    } catch (e) {
      toast("The review could not start", { tone: "danger", body: String(e).replace(/^Error:\s*/, "") });
    } finally {
      setStarting(false);
    }
  };

  const latest = data.reviews[0];
  return (
    <section aria-label="Agent review">
      <div className="flex items-center gap-2">
        <h3 className="text-xs font-semibold text-fg">Agent review</h3>
        <span className="tabular text-xs text-fg-subtle">
          round {Math.min(data.rounds, data.maxRounds)} of {data.maxRounds}
        </span>
        <span className="ml-auto" />
        <Button size="xs" variant="ghost" icon={<RotateCcw className="size-3" />} loading={starting} disabled={busy} onClick={() => void again()}>
          Review again
        </Button>
      </div>
      {!latest ? (
        <p className="mt-1 text-xs text-fg-muted">No review yet. One starts when an agent finishes work on this card.</p>
      ) : (
        <ul className="mt-2 space-y-2">
          {data.reviews.slice(0, 5).map((r) => (
            <li key={r.id} className="rounded-md border border-border bg-panel px-3 py-2">
              <div className="flex items-center gap-2 text-xs">
                {r.verdict === "approve" ? (
                  <CheckCircle2 className="size-3.5 text-success-fg" aria-hidden />
                ) : (
                  <CircleAlert className="size-3.5 text-warning-fg" aria-hidden />
                )}
                <Badge tone={r.verdict === "approve" ? "success" : "warning"}>
                  {r.verdict === "approve" ? "Approved" : r.submitted ? "Changes requested" : "No verdict"}
                </Badge>
                <span className="text-fg-muted">
                  {r.reviewer ?? "Reviewer"} · round {r.round}
                </span>
                <span className="ml-auto text-fg-subtle">{new Date(r.createdAt).toLocaleString()}</span>
              </div>
              {r.summary && <p className="mt-1.5 whitespace-pre-wrap text-xs leading-relaxed text-fg">{r.summary}</p>}
              {r.notes.length > 0 && (
                <ul className="mt-1.5 space-y-1">
                  {r.notes.map((n, i) => (
                    <li key={i} className="text-xs leading-relaxed text-fg-muted">
                      {n.file && (
                        <code className="mr-1 font-mono text-[11px] text-fg">
                          {n.file}
                          {n.line ? `:${n.line}` : ""}
                        </code>
                      )}
                      {n.body}
                    </li>
                  ))}
                </ul>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
