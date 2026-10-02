import { useEffect, useState } from "react";
import { api, Agent, ReviewPolicy } from "../lib/api";
import { useEngines } from "../lib/engines";
import { HistoryButton } from "./RevisionsPanel";
import { Button } from "./ui/Button";
import { Select, Switch } from "./ui/Field";

/**
 * What a card must clear before Merge, and the agent that reviews each run.
 *
 * Merge stays a person's click; this decides what that click asks for. Saved
 * with a button, like checks: the policy gates every card in the project, and
 * a half-made one should never be live.
 */
export function ReviewPolicySettings({ projectId, workspaceId }: { projectId: string; workspaceId: string }) {
  const [draft, setDraft] = useState<ReviewPolicy | null>(null);
  const [saved, setSaved] = useState("");
  const [agents, setAgents] = useState<Agent[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [loads, setLoads] = useState(0);
  const engines = useEngines();

  useEffect(() => {
    api
      .reviewPolicy(projectId)
      .then(({ policy }) => {
        setDraft(policy);
        setSaved(JSON.stringify(policy));
      })
      .catch((e) => setError(String(e)));
  }, [projectId, loads]);
  useEffect(() => {
    api
      .agents(workspaceId)
      .then((r) => setAgents(r.agents))
      .catch(() => setAgents([]));
  }, [workspaceId]);

  if (!draft) return <p className="text-xs text-fg-subtle">{error ?? "Loading…"}</p>;

  const dirty = JSON.stringify(draft) !== saved;
  const set = (patch: Partial<ReviewPolicy>) => setDraft({ ...draft, ...patch });
  // An agent on an engine known not to hold a pass to read-only is shown but
  // cannot be picked: a reviewer that could edit the diff it judges is not a
  // reviewer. Anything not known here is the server's to vet at the save.
  const canReview = (a: Agent) => {
    const engine = engines?.find((e) => e.id === a.engine);
    return !engine || engine.capabilities.enforces_denied_tools;
  };

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      const { policy } = await api.saveReviewPolicy(projectId, draft);
      setDraft(policy);
      setSaved(JSON.stringify(policy));
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  const row = (label: string, hint: string, checked: boolean, onChange: (v: boolean) => void) => (
    <div className="flex items-start gap-3">
      <Switch checked={checked} onChange={onChange} label={label} />
      <div className="min-w-0">
        <div className="text-[13px] text-fg">{label}</div>
        <div className="text-xs leading-relaxed text-fg-muted">{hint}</div>
      </div>
    </div>
  );

  return (
    <div className="space-y-3">
      {row(
        "An agent reviews every run",
        "A different agent reads the diff, read-only, and approves it or asks for changes; each request starts one fix run. Merge then needs its approval of the latest work.",
        draft.requireReview,
        (v) => set({ requireReview: v }),
      )}
      {draft.requireReview && (
        <div className="ml-11 flex flex-wrap items-center gap-2 text-xs text-fg-muted">
          <Select
            className="w-48"
            aria-label="Reviewer"
            value={draft.reviewerAgentId ?? ""}
            onChange={(e) => set({ reviewerAgentId: e.target.value || null })}
          >
            <option value="">Pick a reviewer…</option>
            {agents.map((a) => (
              <option key={a.id} value={a.id} disabled={!canReview(a)}>
                {a.name}
                {canReview(a) ? "" : " — its engine cannot be held to read-only"}
              </option>
            ))}
          </Select>
          <Select
            className="w-36"
            aria-label="Rounds"
            value={draft.maxRounds}
            onChange={(e) => set({ maxRounds: Number(e.target.value) })}
          >
            <option value={1}>1 round</option>
            <option value={2}>up to 2 rounds</option>
            <option value={3}>up to 3 rounds</option>
          </Select>
          <span>then it waits for you.</span>
        </div>
      )}
      {row(
        "Merge needs passing checks",
        "The latest checks, run on the latest work. Older green checks do not count.",
        draft.requireChecks,
        (v) => set({ requireChecks: v }),
      )}
      {row(
        "Run checks after every run",
        "Not only after Full Auto. Checks execute code an agent may have edited — turning this on is your standing consent to that.",
        draft.runChecksAfterEveryRun,
        (v) => set({ runChecksAfterEveryRun: v }),
      )}
      {row(
        "Merge needs a green pull request",
        "GitHub is asked again at the click. A card with no pull request, or checks still running, waits.",
        draft.requirePrGreen,
        (v) => set({ requirePrGreen: v }),
      )}

      {error && <p className="rounded-md bg-danger-subtle px-3 py-2 text-xs text-danger-fg">{error}</p>}
      <div className="flex items-center gap-2">
        <span className="text-xs text-fg-subtle">You can always merge past it, with a note saying why.</span>
        <span className="ml-auto" />
        <HistoryButton kind="review_policy" id={projectId} onRestored={() => setLoads((n) => n + 1)} />
        <Button size="sm" variant="primary" disabled={!dirty} loading={busy} onClick={() => void save()}>
          {dirty ? "Save policy" : "Saved"}
        </Button>
      </div>
    </div>
  );
}
