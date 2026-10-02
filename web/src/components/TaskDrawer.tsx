import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Agent, api, Attachment, CheckoutState, displayTier, MergeGateError, PendingPermission, Skill, Task, Team, tierColor, type Unmet } from "../lib/api";
import { useRunStream, StreamEvent } from "../lib/ws";
import { isActive, isWorking, statusLabel, stopReason, unresolvedBlockers } from "../lib/runStatus";
import { estimateLine, ForecastAsk, parseForecastAsk } from "../lib/forecast";
import { useAttachments } from "../lib/useAttachments";
import { AttachmentBar, AttachmentList } from "./AttachmentBar";
import { TaskComments } from "./TaskComments";
import { Markdown } from "./Markdown";
import { annotateDiff, hunkText, isCommentable } from "../lib/diff";
import { PermissionRow } from "./PermissionRow";
import { BakeoffView } from "./BakeoffView";
import { RunStream, ActivityLine } from "./RunStream";
import { AssigneePicker } from "./AssigneePicker";
import { SkillPicker } from "./SkillPicker";
import { PlanReviewPanel } from "./PlanReviewPanel";
import { ArticlePicker } from "./kb/ArticlePicker";
import { useTierModel } from "../lib/models";
import { EnginePicker, useEngines } from "../lib/engines";
import { PreviewPanel } from "./PreviewPanel";
import { CardTierPicker } from "./TierPicker";
import { EffortPicker } from "./EffortPicker";
import { PullRequestPanel } from "./PullRequestPanel";
import { RunHistory } from "./RunHistory";
import { ChecksPanel } from "./ChecksPanel";
import { BaseStatus } from "./BaseStatus";
import { parseMergeRefusal } from "../lib/mergeRefusal";
import { RunError } from "./ui/RunError";
import { springy } from "../lib/motion";
import { Badge, StatusDot, type Tone } from "./ui/Badge";
import { Avatar } from "./ui/Avatar";
import { Button, IconButton } from "./ui/Button";
import { Menu } from "./ui/Overlay";
import { Tabs, TabPanel } from "./ui/Tabs";
import { Timeline } from "./task/Timeline";
import { ReviewPanel } from "./task/ReviewPanel";
import { HandOff } from "./task/HandOff";
import { GoalPicker } from "./GoalPicker";
import { Textarea } from "./ui/Field";
import { Building2, FileDiff, GitMerge, MoreHorizontal, Play, RotateCcw, Scale, Square, Trash2, X } from "lucide-react";

type DrawerTab = "overview" | "activity" | "comments" | "checks" | "history" | "diff" | "bakeoff";

/** A run status as a tone: live is accent, parked is warning, failed is danger. */
function runTone(status: string): Tone {
  if (status === "failed") return "danger";
  if (status === "completed") return "success";
  if (status === "awaiting_approval" || status === "waiting_permission" || status === "rate_limited") return "warning";
  if (status === "canceled") return "neutral";
  return "accent";
}

export function TaskDrawer({
  onOpenPreviews,
  task,
  workspaceId,
  onClose,
  onChanged,
  onOpenTeamRoom,
  boardTasks = [],
  onOpenTask,
}: {
  task: Task;
  /** Bounds which agents a bake-off may choose between. */
  workspaceId: string;
  onClose: () => void;
  onChanged: () => void;
  onOpenTeamRoom?: (runId: string) => void;
  /** The project's cards, so an epic can list its own without a second fetch. */
  boardTasks?: Task[];
  onOpenTask?: (t: Task) => void;
  /** Switch the project page to its Previews tab, which owns the detail. */
  onOpenPreviews?: () => void;
}) {
  const tierModel = useTierModel();
  const engines = useEngines();
  // An earlier run picked from History, replayed in the Activity tab. Null is
  // "the card's newest run", which is what everything live — Cancel, the
  // permission prompts, the plan panel — stays bound to regardless.
  const [viewing, setViewing] = useState<string | null>(null);
  useEffect(() => setViewing(null), [task.id, task.runId]);
  // The drawer is reused when another card is picked. Anything pinned about
  // the last card — a refused merge, the gate it hit, a half-written note to
  // merge anyway — must not carry over, or its buttons act on the new card.
  useEffect(() => {
    setGate(null);
    setOverride("");
    setConfirm(null);
    setBlocked(null);
    setConflicted(false);
    setError(null);
  }, [task.id]);
  const events = useRunStream(viewing ?? task.runId);
  const [diff, setDiff] = useState<string | null>(null);
  // Whether the Diff tab is chosen — separate from whether the diff has
  // arrived, so the tab selects at the click and shows that it is loading,
  // and a slow fetch landing later never pulls the person back to it.
  const [diffOpen, setDiffOpen] = useState(false);
  const diffReq = useRef(0);
  // The bake-off panel: same brief, several attempts, compare and keep one.
  const [bakeoff, setBakeoff] = useState(false);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [skills, setSkills] = useState<Skill[]>([]);
  const [teams, setTeams] = useState<Team[]>([]);
  const [reassignError, setReassignError] = useState<string | null>(null);
  const [merging, setMerging] = useState(false);
  // What the merge guard refused over, once it has refused. Null until then —
  // asking the server what is dirty before anything has gone wrong would be a
  // request per drawer open for a question nobody asked.
  const [blocked, setBlocked] = useState<CheckoutState | null>(null);
  const [resolving, setResolving] = useState<"stash" | "commit" | null>(null);
  // Merge was refused because the branch conflicts with the base — the cue to
  // offer bringing the base in and having the conflict resolved here.
  const [conflicted, setConflicted] = useState(false);
  const [serverPending, setServerPending] = useState<PendingPermission[]>([]);
  const [answered, setAnswered] = useState<Set<string>>(new Set());
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [articleIds, setArticleIds] = useState<string[]>([]);
  // A live run opens on its transcript, not on an empty comment thread —
  // landing on "No comments yet" while an agent is mid-Bash is how the card
  // ends up looking like nothing is happening at all.
  const [panel, setPanel] = useState<"overview" | "comments" | "activity" | "history" | "checks">(
    isActive(task.runStatus) ? "activity" : "overview",
  );
  const [historyView, setHistoryView] = useState<"runs" | "timeline">("runs");
  const att = useAttachments(task.projectId);
  const [attachBusy, setAttachBusy] = useState(false);
  const [busy, setBusy] = useState<"retry" | "resume" | "delete" | null>(null);
  const [error, setError] = useState<string | null>(null);
  // What the project's review policy still wants before this card may land,
  // after a merge it refused — and the note a person writes to merge anyway.
  const [gate, setGate] = useState<Unmet[] | null>(null);
  const [override, setOverride] = useState("");
  const [confirm, setConfirm] = useState<{
    title: string;
    body: string;
    cta: string;
    go: () => void;
  } | null>(null);
  const shownTier = displayTier(task);
  const accent = tierColor[shownTier];
  // Anything that still owes an outcome, including a team run parked for
  // your approval — those must not look finished.
  const running = isActive(task.runStatus);

  // `fresh` is the whole difference between the two Retries and was hardcoded
  // to `true`, so the non-destructive one — implemented server-side since the
  // route was written — had no way to be asked for.
  const doRetry = async (fresh: boolean) => {
    setConfirm(null);
    setBusy("retry");
    try {
      await api.retryTask(task.id, fresh);
      onChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const resume = async () => {
    if (!task.runId) return;
    setBusy("resume");
    setError(null);
    try {
      await api.resumeRun(task.runId);
      onChanged();
    } catch (e) {
      // A refusal arrives here as a 409 saying which one — the engine can't
      // resume, or the worktree has been reclaimed. Those two can't be
      // answered by the board query, so this banner is where they land.
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const retry = () => {
    // A card in review holds an unmerged diff, and a fresh retry throws it
    // away — that is worth one click of confirmation.
    if (task.boardColumn === "review") {
      setConfirm({
        title: "Retry discards the current diff",
        body: "This card has unmerged work. Retrying starts again from a clean checkout, so that diff is lost.",
        cta: "Retry anyway",
        go: () => doRetry(true),
      });
    } else {
      doRetry(true);
    }
  };

  const remove = () => {
    setConfirm({
      title: "Delete this card?",
      body: "Its comments, run history, attachments, and worktree branch go with it. Agents keep what they remember about the work.",
      cta: "Delete",
      go: async () => {
        setConfirm(null);
        setBusy("delete");
        try {
          await api.deleteTask(task.id);
          onChanged();
          onClose();
        } catch (e) {
          setError(String(e));
          setBusy(null);
        }
      },
    });
  };

  useEffect(() => {
    api
      .taskArticles(task.id)
      .then((r) => setArticleIds(r.articles.map((a) => a.id)))
      .catch(() => {});
  }, [task.id]);

  useEffect(() => {
    // Every agent, retired ones included, so the card's own assignee is
    // always nameable; the picker and the bake-off offer only working ones.
    api
      .allAgents(workspaceId)
      .then((r) => setAgents(r.agents))
      .catch(() => {});
    api
      .teams(workspaceId)
      .then((r) => setTeams(r.teams))
      .catch(() => {});
    api
      .skills(workspaceId)
      .then((r) => setSkills(r.skills))
      .catch(() => {});
  }, [workspaceId]);

  // Follow a run that starts while the drawer is already open. Keyed on the
  // run id so it fires once per run and never fights a manual tab click.
  const followedRun = useRef<string | null>(null);
  useEffect(() => {
    if (task.runId && isActive(task.runStatus) && followedRun.current !== task.runId) {
      followedRun.current = task.runId;
      setPanel("activity");
    }
  }, [task.runId, task.runStatus]);

  const reassign = async (next: { kind: "agent" | "team"; id: string } | null) => {
    setReassignError(null);
    try {
      await api.reassignTask(task.id, next);
      onChanged();
    } catch (e) {
      setReassignError(String(e).replace(/^Error:\s*/, ""));
    }
  };

  useEffect(() => {
    setAttachments([]);
    api
      .taskAttachments(task.id)
      .then((r) => setAttachments(r.attachments))
      .catch(() => {});
  }, [task.id]);

  // Bind freshly-uploaded files to this card; its next run will see them.
  const commitAttachments = async () => {
    if (!att.ids.length || attachBusy) return;
    setAttachBusy(true);
    try {
      await api.attachToTask(task.id, att.ids);
      att.clear();
      const r = await api.taskAttachments(task.id);
      setAttachments(r.attachments);
    } catch {
      /* chips keep their state; user can retry */
    } finally {
      setAttachBusy(false);
    }
  };

  // Permission requests are held in memory by the broker while the engine
  // blocks on them, so a refresh has to re-fetch whatever is still open.
  const runId = task.runId;
  const refreshPending = useCallback(async () => {
    if (!runId) return setServerPending([]);
    try {
      setServerPending((await api.pendingPermissions(runId)).pending);
    } catch {
      /* transient; next tick retries */
    }
  }, [runId]);

  useEffect(() => {
    setAnswered(new Set());
    refreshPending();
    const interval = setInterval(refreshPending, 3000);
    return () => clearInterval(interval);
  }, [refreshPending]);

  // Open prompts = server-held ∪ live-streamed, minus resolved/answered.
  const openPermissions = useMemo(() => {
    const resolved = new Set(
      events
        .filter((e) => e.type === "permission_resolved")
        .map((e) => String(e.request_id)),
    );
    const merged = new Map<string, PendingPermission>();
    for (const p of serverPending) merged.set(p.requestId, p);
    for (const e of events) {
      if (e.type !== "permission_requested") continue;
      const requestId = String(e.request_id);
      merged.set(requestId, {
        requestId,
        toolName: String(e.tool_name),
        input: e.input,
      });
    }
    return [...merged.values()].filter(
      (p) => !resolved.has(p.requestId) && !answered.has(p.requestId),
    );
  }, [events, serverPending, answered]);

  const answer = async (requestId: string, allowed: boolean) => {
    setAnswered((prev) => new Set(prev).add(requestId));
    try {
      await api.resolvePermission(requestId, allowed);
    } finally {
      refreshPending();
    }
  };

  const loadDiff = () => {
    const req = ++diffReq.current;
    setDiff(null);
    api
      .diff(task.id)
      .then((r) => {
        if (diffReq.current === req) setDiff(r.diff);
      })
      .catch((e) => {
        if (diffReq.current !== req) return;
        setError(`Could not load the diff. ${String(e).replace(/^Error:\s*/, "")}`);
        setDiffOpen(false);
      });
  };
  // Failing checks warn, they do not block: the person may know the failure
  // is old, flaky, or not this card's — but they should know it is there.
  const merge = () => {
    const c = task.localChecks;
    if (c?.status === "failed") {
      setConfirm({
        title: "This card's checks fail",
        body: `${c.total - c.passed} of ${c.total} of this project's checks fail on this card. Merging lands it anyway.`,
        cta: "Merge anyway",
        go: () => {
          setConfirm(null);
          doMerge();
        },
      });
    } else {
      doMerge();
    }
  };

  const doMerge = async (force?: { note: string }) => {
    if (merging) return;
    setMerging(true);
    setError(null);
    setBlocked(null);
    try {
      await api.merge(task.id, force);
      setGate(null);
      onChanged();
      onClose();
    } catch (e) {
      if (e instanceof MergeGateError) {
        setGate(e.unmet);
        return;
      }
      // Inline, like every other failure in this drawer. A native alert()
      // loses the drawer's context and can't be copied out of easily.
      const raw = String(e).replace(/^Error:\s*/, "");
      const refusal = parseMergeRefusal(raw);
      const text = refusal?.error ?? raw;
      setError(`Merge failed. ${text}`);
      const conflict = refusal?.kind === "conflict" || refusal?.kind === "markers";
      setConflicted(conflict);
      // "Update from main" — the remedy the message names — is on Checks.
      if (conflict) setTab("checks");
      // The one refusal with something to do about it. The guard names the
      // files in prose; fetching them as data is what lets the buttons below
      // exist, and it asks the same endpoint the guard reads so the list
      // cannot disagree with what is actually in the way.
      if (text.includes("uncommitted changes")) {
        api.projectCheckout(task.projectId).then(setBlocked).catch(() => {});
      }
    } finally {
      setMerging(false);
    }
  };

  /// Clear the way, then let them press Merge again.
  ///
  /// Deliberately not an auto-retry: folding a remedy into the merge is exactly
  /// the silent behaviour the guard was written to stop.
  const clearTheWay = async (how: "stash" | "commit") => {
    setResolving(how);
    try {
      const r =
        how === "stash"
          ? await api.stashCheckout(task.projectId)
          : await api.commitCheckout(task.projectId);
      setBlocked(null);
      setError(
        how === "stash"
          ? `Set aside. Your changes are in the stash — \`${r.undo}\` brings them back. Merge again when you're ready.`
          : `Committed as its own commit, separate from this card's. \`${r.undo}\` undoes it. Merge again when you're ready.`,
      );
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setResolving(null);
    }
  };

  // Which tab is showing. The board's urgent things — permission prompts, a
  // refused merge, a confirmation — are pinned above the tabs, never inside
  // one, so they cannot be missed by looking at the wrong tab.
  const hasChecks =
    task.boardColumn === "review" || task.boardColumn === "done" || !!task.localChecks;
  const tab: DrawerTab = bakeoff ? "bakeoff" : diffOpen ? "diff" : panel;
  function setTab(t: DrawerTab) {
    if (t !== "bakeoff") setBakeoff(false);
    setDiffOpen(t === "diff");
    if (t !== "diff") {
      diffReq.current++;
      setDiff(null);
    }
    if (t === "diff") loadDiff();
    else if (t === "overview" || t === "comments" || t === "activity" || t === "history" || t === "checks") setPanel(t);
  }
  const statusTone = task.runStatus ? runTone(task.runStatus) : "neutral";
  const cost =
    (task.runCount ?? 0) > 1 && task.totalCostUsd != null
      ? { text: `$${task.totalCostUsd.toFixed(3)}`, title: `over ${task.runCount} runs — latest $${(task.costUsd ?? 0).toFixed(3)}` }
      : task.costUsd != null
        ? { text: `$${task.costUsd.toFixed(3)}`, title: undefined }
        : null;

  return (
    <motion.aside
      initial={{ x: 600 }}
      animate={{ x: 0 }}
      exit={{ x: 600 }}
      transition={{ type: "spring", stiffness: 340, damping: 36 }}
      aria-label={`Card: ${task.title}`}
      className="fixed inset-y-0 right-0 z-30 flex w-full max-w-[600px] flex-col border-l border-border bg-panel shadow-[var(--shadow-lg)]"
    >
      <header className="shrink-0 border-b border-border px-4 pb-3 pt-3.5">
        <div className="flex items-start gap-3">
          <div className="min-w-0 flex-1">
            <h2 className="text-[15px] font-semibold leading-snug text-fg">{task.title}</h2>
            <div className="mt-1.5 flex flex-wrap items-center gap-1.5 text-xs text-fg-muted">
              <Badge tone={shownTier}>
                {task.tierIsAuto && "auto · "}
                {tierModel(shownTier)}
              </Badge>
              {task.runStatus && (
                <Badge tone={statusTone} icon={<StatusDot tone={statusTone} pulse={running} className="size-1.5" />}>
                  {statusLabel(task.runStatus)}
                </Badge>
              )}
              {task.agentName && (
                <span className="flex items-center gap-1">
                  <Avatar name={task.agentName} color={task.agentColor} size={16} />
                  {task.agentName}
                </span>
              )}
              {cost && (
                <span className="tabular ml-auto font-mono" title={cost.title}>
                  {cost.text}
                </span>
              )}
            </div>
            {/* Why Eren picked this tier, whenever Eren did the picking — a
                choice made on someone's behalf that they cannot see is the
                silent downgrade this project refuses elsewhere. */}
            {task.tierIsAuto && task.tierReason && (
              <div className="mt-1 text-[11px] text-fg-muted">
                Auto → {task.tierResolved}: {task.tierReason}
              </div>
            )}
            {(() => {
              const stopped = stopReason(task.runStatus, task.runError);
              return stopped ? <RunError reason={stopped.text} tone={stopped.tone} className="mt-2" /> : null;
            })()}
            <ActivityLine events={viewing ? [] : events} live={running && !viewing} className="mt-1" />
          </div>
          <IconButton label="Close" onClick={onClose}>
            <X className="size-4" />
          </IconButton>
        </div>

        <div className="mt-3 flex flex-wrap items-center gap-1.5">
          {task.boardColumn === "review" &&
            (task.prState === "merged" ? (
              // Once it is merged on GitHub, squash-merging would write the
              // same change again under this card's message. What is needed
              // is a pull, and saying so beats a button that duplicates.
              <span className="text-xs text-fg-muted">
                Merged on GitHub — <code className="font-mono">git pull</code> to update your checkout.
              </span>
            ) : (
              <Button size="sm" variant="primary" icon={<GitMerge className="size-3.5" />} loading={merging} onClick={merge}>
                Squash-merge
              </Button>
            ))}
          {/* Resume before Retry: cheaper, and what people want after a run
              dies forty minutes in. Only when there is a session to pick up. */}
          {!running && task.runResumable && (
            <Button
              size="sm"
              variant={task.boardColumn === "review" ? "secondary" : "primary"}
              icon={<Play className="size-3.5" />}
              loading={busy === "resume"}
              disabled={busy !== null}
              onClick={resume}
              title="Continue the same session, in the same worktree, from where it stopped"
            >
              Resume
            </Button>
          )}
          {!running && (
            <Button
              size="sm"
              icon={<RotateCcw className="size-3.5" />}
              loading={busy === "retry"}
              disabled={busy !== null}
              onClick={retry}
              title="Run this card again from a clean checkout"
            >
              Retry
            </Button>
          )}
          {task.runId && isWorking(task.runStatus) && (
            <Button size="sm" variant="danger" icon={<Square className="size-3" />} onClick={() => api.cancelRun(task.runId!)}>
              Cancel run
            </Button>
          )}
          {task.orgRunId && onOpenTeamRoom && (
            <Button size="sm" icon={<Building2 className="size-3.5" />} onClick={() => onOpenTeamRoom(task.orgRunId!)}>
              Team room
            </Button>
          )}
          <div className="ml-auto flex items-center gap-1">
            <Menu
              align="end"
              trigger={
                <IconButton label="More actions">
                  <MoreHorizontal className="size-4" />
                </IconButton>
              }
              items={[
                // A bake-off answers "which agent should do this?" with
                // evidence, so it belongs before the work is accepted.
                ...(!task.teamId && task.boardColumn !== "done"
                  ? [{ label: "Bake-off", icon: <Scale className="size-3.5" />, onSelect: () => setTab("bakeoff") }]
                  : []),
                ...(task.boardColumn === "review"
                  ? [{ label: "View diff", icon: <FileDiff className="size-3.5" />, onSelect: () => setTab("diff") }]
                  : []),
                null,
                { label: "Delete card", icon: <Trash2 className="size-3.5" />, danger: true, disabled: busy !== null, onSelect: remove },
              ]}
            />
          </div>
        </div>
        {/* Merging what an agent left half-finished reads like accepting
            finished work unless it is said. */}
        {task.boardColumn === "review" &&
          task.prState !== "merged" &&
          (task.runStatus === "failed" || task.runStatus === "canceled") && (
            <p className="mt-2 text-[11px] text-warning-fg">
              This run {task.runStatus === "failed" ? "failed" : "was cancelled"} — merging lands whatever it got to.
            </p>
          )}
        {!running && task.runResumable && (
          <p className="mt-1.5 text-[11px] leading-snug text-fg-muted">
            Resume continues where it stopped. Retry starts over from a clean checkout.
          </p>
        )}
      </header>

      {/* Everything pinned above the tabs scrolls as one, and never takes more
          than half the drawer: several prompts stacked up is exactly when you
          most need to reach all of them, and the tabs below must stay. */}
      <div className="max-h-[50vh] shrink-0 overflow-y-auto">
        <AnimatePresence>
          {openPermissions.length > 0 && (
            <motion.div
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: "auto", opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              className="overflow-hidden border-b border-border bg-warning-subtle"
            >
              <div className="flex flex-col gap-2 p-4">
                {openPermissions.map((p) => (
                  <PermissionRow
                    key={p.requestId}
                    toolName={p.toolName}
                    input={p.input}
                    onAnswer={(allowed) => answer(p.requestId, allowed)}
                  />
                ))}
              </div>
            </motion.div>
          )}
        </AnimatePresence>
        {/* `whitespace-pre-wrap`: the merge guard formats the blocking files one
            per line, and without this they arrived as a single run-on sentence.
            Same treatment app build errors already get. */}
        {error && (
          <div className="whitespace-pre-wrap border-b border-border bg-danger-subtle px-4 py-2 text-xs leading-relaxed text-danger-fg">
            {error}
          </div>
        )}

        {blocked && blocked.dirty.length > 0 && (
          <div className="border-b border-border bg-warning-subtle px-5 py-3">
            <div className="text-xs font-medium text-warning-fg">
              {blocked.dirty.length === 1
                ? "One file in your checkout is in the way"
                : `${blocked.dirty.length} files in your checkout are in the way`}
              {blocked.branch && <span className="font-normal"> — on {blocked.branch}</span>}
            </div>
            <ul className="mt-1.5 max-h-40 overflow-y-auto">
              {blocked.dirty.map((f) => (
                <li key={f.path} className="flex items-baseline gap-2 font-mono text-[11px] text-warning-fg/90">
                  <span className="w-4 shrink-0 text-warning-fg">{`${f.index}${f.worktree}`.trim()}</span>
                  <span className="truncate">{f.path}</span>
                </li>
              ))}
            </ul>
            <div className="mt-2 flex flex-wrap items-center gap-2">
              <Button size="sm" onClick={() => clearTheWay("stash")} disabled={resolving !== null}>
                {resolving === "stash" ? "Setting aside…" : "Stash them"}
              </Button>
              <Button size="sm" onClick={() => clearTheWay("commit")} disabled={resolving !== null}>
                {resolving === "commit" ? "Committing…" : "Commit them"}
              </Button>
              {/* Which one to press is a real choice, so say what each does
                  rather than leaving it to be discovered. */}
              <span className="text-[11px] text-warning-fg/80">
                Stashing sets them aside; committing keeps them, in their own commit.
              </span>
            </div>
          </div>
        )}

        {gate && (
          <div className="border-b border-border bg-warning-subtle px-4 py-3 text-xs text-warning-fg">
            <div className="font-medium">This project's review policy still wants:</div>
            <ul className="mt-1 list-disc space-y-0.5 pl-4">
              {gate.map((u) => (
                <li key={u.kind}>{u.message}</li>
              ))}
            </ul>
            <Textarea
              className="mt-2 min-h-[52px] text-fg"
              value={override}
              maxLength={500}
              onChange={(e) => setOverride(e.target.value)}
              placeholder="To merge anyway, say why. It goes on the card and in the audit log."
              aria-label="Why merge anyway"
            />
            <div className="mt-2 flex gap-2">
              <Button
                size="sm"
                variant="danger"
                loading={merging}
                disabled={!override.trim()}
                onClick={() => void doMerge({ note: override.trim() })}
              >
                Merge anyway
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setGate(null)}>
                Cancel
              </Button>
            </div>
          </div>
        )}

        {confirm && (
          <div className="border-b border-border bg-warning-subtle px-5 py-3 text-xs text-warning-fg">
            <div className="font-medium">{confirm.title}</div>
            <div className="mt-0.5">{confirm.body}</div>
            <div className="mt-2 flex gap-2">
              <Button size="sm" variant="danger" onClick={confirm.go}>
                {confirm.cta}
              </Button>
              <Button size="sm" variant="ghost" onClick={() => setConfirm(null)}>
                Cancel
              </Button>
            </div>
          </div>
        )}
      </div>

      <Tabs<DrawerTab>
        className="min-h-0 flex-1"
        value={tab}
        onValueChange={setTab}
        tabs={[
          { value: "overview", label: "Overview" },
          {
            value: "activity",
            label: "Activity",
            badge: running ? <StatusDot tone="accent" pulse className="size-1.5" /> : undefined,
          },
          { value: "comments", label: "Comments" },
          ...(hasChecks ? [{ value: "checks" as const, label: "Checks" }] : []),
          {
            value: "history",
            label: "History",
            badge: (task.runCount ?? 0) > 1 ? <span className="tabular text-[11px] text-fg-muted">{task.runCount}</span> : undefined,
          },
          ...(task.boardColumn === "review" || diffOpen ? [{ value: "diff" as const, label: "Diff" }] : []),
          ...(bakeoff ? [{ value: "bakeoff" as const, label: "Bake-off" }] : []),
        ]}
      >
        <TabPanel value="overview" className="overflow-y-auto">
      <StatusMover task={task} running={running} onChanged={onChanged} />
      <Blockers task={task} boardTasks={boardTasks} onChanged={onChanged} onOpenTask={onOpenTask} />
      <Description task={task} running={running} onChanged={onChanged} />
      <EpicPanel task={task} boardTasks={boardTasks} onOpenTask={onOpenTask} />
      {task.runId && <PlanReviewPanel runId={task.runId} onChanged={onChanged} />}

      <Setup
        // Configure-time fields earn the space only at configure time: a card
        // that has already run is being read, not set up, so it opens folded
        // to a one-line summary.
        defaultOpen={task.boardColumn === "backlog" && !task.runId}
        summary={[
          task.teamName ?? task.agentName ?? "unassigned",
          tierModel(shownTier),
          task.skillName ?? null,
          task.effectiveEffort ? `${task.effectiveEffort} thinking` : null,
        ]
          .filter(Boolean)
          .join(" · ")}
      >
        <div className="mb-1.5 text-xs font-medium text-fg-muted">
          Assigned to
        </div>
        <AssigneePicker
          value={
            task.teamId
              ? { kind: "team", id: task.teamId }
              : task.agentId
                ? { kind: "agent", id: task.agentId }
                : null
          }
          agents={agents}
          teams={teams}
          disabled={running}
          disabledReason="Cancel the run, or hand it off with a note below."
          onChange={reassign}
        />
        {running && !task.teamId && (
          <HandOff taskId={task.id} current={task.agentId ?? null} agents={agents} onDone={onChanged} />
        )}
        {reassignError && (
          <div className="mt-1.5 rounded-md bg-danger-subtle px-2.5 py-1.5 text-[11px] text-danger-fg">
            {reassignError}
          </div>
        )}

        {/* Beside who, not inside it: the two compose. Hidden until the
            workspace has one, and shown regardless once this card carries
            one — a card pointing at a skill has to say so. */}
        {(skills.some((s) => s.enabled) || task.skillId) && (
          <div className="mt-3">
            <div className="mb-1.5 text-xs font-medium text-fg-muted">
              How
            </div>
            <SkillPicker
              value={task.skillId ?? null}
              skills={skills}
              disabled={running}
              disabledReason="Cancel the run to change how this card is done."
              onChange={async (next) => {
                await api.moveTask(task.id, { skill_id: next });
                onChanged();
              }}
            />
          </div>
        )}
        {/* What the work is for: every run of this card is told the goal
            chain. Hidden until the workspace has goals. */}
        <div className="mt-3 empty:hidden">
          <GoalPicker
            workspaceId={workspaceId}
            value={task.goalId ?? null}
            onChange={async (goal) => {
              await api.moveTask(task.id, { goal_id: goal });
              onChanged();
            }}
          />
        </div>
        <div className="mt-3">
          <ArticlePicker
            workspaceId={workspaceId}
            selected={articleIds}
            onChange={async (ids) => {
              setArticleIds(ids);
              await api.setTaskArticles(task.id, ids);
            }}
          />
        </div>

        <label className="mt-3 flex cursor-pointer items-start gap-2 text-xs">
          <input
            type="checkbox"
            checked={task.planFirst}
            disabled={running}
            onChange={async (e) => {
              await api.moveTask(task.id, { plan_first: e.target.checked });
              onChanged();
            }}
            className="mt-0.5 accent-[var(--color-accent)]"
          />
          <span className="min-w-0">
            <span className="block font-medium">Plan first</span>
            <span className="block text-[11px] text-fg-muted">
              Write a plan and stop, so you can confirm or rewrite it before
              anything changes.
            </span>
          </span>
        </label>

        {!!engines && engines.length > 1 && (
          <div className="mt-3 flex items-center gap-2">
            <span className="text-xs font-medium text-fg-muted">
              Run on
            </span>
            <EnginePicker
              value={task.engine}
              onChange={async (id) => {
                if (!id || running) return;
                await api.moveTask(task.id, { engine: id });
                onChanged();
              }}
            />
          </div>
        )}

        <div className="mt-3 flex flex-wrap items-center gap-2">
          <span className="text-xs font-medium text-fg-muted">
            Model
          </span>
          <CardTierPicker
            value={task.modelTier}
            engine={task.engine}
            disabled={running}
            onChange={async (t) => {
              await api.moveTask(task.id, { model_tier: t });
              onChanged();
            }}
          />
          <span className="text-xs font-medium text-fg-muted">
            Thinking
          </span>
          <EffortPicker
            value={task.effort}
            disabled={running}
            // Only worth naming when it comes from somewhere other than this
            // card — otherwise "Default (medium)" beside a card that says
            // medium reads as if it were set twice.
            inherited={task.effortSource === "card" ? null : task.effectiveEffort}
            onChange={async (e) => {
              await api.moveTask(task.id, { effort: e });
              onChanged();
            }}
          />
          {/* Where "Default" actually came from. Silent when the card sets it
              itself, since the picker already says so. */}
          {task.effortSource === "agent" && (
            <span className="text-[11px] text-fg-muted">
              {task.agentName
                ? `set by the ${task.agentName} agent, which outranks this card`
                : "set by its agent, which outranks this card"}
            </span>
          )}
          {task.effortSource === "tier" && (
            <span className="text-[11px] text-fg-muted">
              from the {task.modelTier} tier on {task.engine}
            </span>
          )}
        </div>

        <Permissions task={task} />
      </Setup>

      <div className="border-b border-border px-4 py-1 empty:hidden">
        <PreviewPanel
          taskId={task.id}
          projectId={task.projectId}
          onOpenPreviews={onOpenPreviews}
        />
      </div>

      <div className="border-b border-border px-4 py-3">
        <div className="mb-2 text-xs font-medium text-fg-muted">
          Attachments
        </div>
        <AttachmentList attachments={attachments} />
        <div className="flex items-center gap-2">
          <AttachmentBar
            items={att.items}
            onAdd={att.add}
            onRemove={att.remove}
            full={att.full}
          />
          {att.ids.length > 0 && (
            <Button variant="primary" size="sm" onClick={commitAttachments} disabled={att.busy || attachBusy}>
              {attachBusy ? "Attaching…" : `Attach ${att.ids.length}`}
            </Button>
          )}
        </div>
      </div>

        </TabPanel>
        <TabPanel value="activity" className="overflow-y-auto p-4">
          {viewing && (
            <div className="mb-3 flex items-center justify-between rounded-md bg-panel-2 px-3 py-2 text-[11px] text-fg-muted">
              <span>Showing an earlier run of this card.</span>
              <Button variant="link" size="xs" onClick={() => setViewing(null)}>
                Back to the latest
              </Button>
            </div>
          )}
          <RunStream events={events} empty="Nothing yet." />
        </TabPanel>
        <TabPanel value="comments" className="overflow-y-auto p-4">
          <TaskComments taskId={task.id} />
        </TabPanel>
        {hasChecks && (
          <TabPanel value="checks" className="overflow-y-auto">
      {task.boardColumn === "review" && (
        <div className="border-b border-border px-4 py-2 empty:hidden">
          <BaseStatus
            taskId={task.id}
            busy={running}
            refreshKey={`${task.runId}:${task.runStatus}`}
            conflicted={conflicted}
            onChanged={() => {
              setConflicted(false);
              setError(null);
              onChanged();
            }}
          />
        </div>
      )}

      {/* Before the pull request: whether the work passes is the first thing
          to know about a diff you are deciding whether to land. */}
      {(task.boardColumn === "review" || (task.localChecks && task.boardColumn !== "done")) && (
        <div className="border-b border-border px-4 py-2">
          <ChecksPanel
            taskId={task.id}
            busy={running}
            refreshKey={`${task.runId}:${task.runStatus}:${task.localChecks?.status ?? ""}`}
            onChanged={onChanged}
          />
        </div>
      )}

      {(task.boardColumn === "review" || task.boardColumn === "done") && (
        <div className="border-b border-border px-4 py-2 empty:hidden">
          <ReviewPanel taskId={task.id} busy={running} refreshKey={`${task.runId}:${task.runStatus}`} />
        </div>
      )}

      {/* Below the row rather than in it: the status line wants the full
          width, and a card keeps its pull request after it leaves review. */}
      {(task.boardColumn === "review" || task.boardColumn === "done") && (
        <div className="border-b border-border px-4 py-2">
          <PullRequestPanel
            taskId={task.id}
            projectId={task.projectId}
            onPublished={onChanged}
          />
        </div>
      )}

          </TabPanel>
        )}
        <TabPanel value="history" className="overflow-y-auto p-4">
          <div className="mb-3">
            <Tabs<"runs" | "timeline">
              variant="pill"
              value={historyView}
              onValueChange={setHistoryView}
              tabs={[
                { value: "runs", label: "Runs" },
                { value: "timeline", label: "Timeline" },
              ]}
            />
          </div>
          {historyView === "timeline" ? (
            <Timeline taskId={task.id} refreshKey={`${task.runId}:${task.runStatus}`} />
          ) : (
          <RunHistory
            taskId={task.id}
            latestRunId={task.runId}
            latestStatus={task.runStatus}
            viewing={viewing}
            onView={(id) => {
              setViewing(id === task.runId ? null : id);
              setPanel("activity");
            }}
          />
          )}
        </TabPanel>
        <TabPanel value="diff" className="overflow-y-auto p-4">
          {diff === null ? (
            <div className="text-xs text-fg-muted">Loading the diff…</div>
          ) : (
            <DiffView diff={diff} taskId={task.id} onBack={() => setTab("overview")} onFixStarted={onChanged} />
          )}
        </TabPanel>
        <TabPanel value="bakeoff" className="overflow-y-auto p-4">
          {bakeoff && (
            <BakeoffView
              taskId={task.id}
              agents={agents.filter((a) => a.status !== "retired")}
              currentTier={shownTier}
              onKept={onChanged}
              onClose={() => setTab("overview")}
            />
          )}
        </TabPanel>
      </Tabs>
    </motion.aside>
  );
}

/**
 * What this card will stop to ask about, and who decided that.
 *
 * It reads as trivia until it isn't. The permission mode is resolved from three
 * places — the bound agent's preset, then the card's, then the machine default —
 * and the first one wins. So a project switched to "works without asking" goes on
 * prompting for every command if its agent carries `reviewed`, and nothing
 * anywhere said which of the three was in charge. The answer to "why is it still
 * asking me" was previously a database query.
 */
function Permissions({ task }: { task: Task }) {
  const says = {
    reviewed: "Asks before editing files or running commands",
    auto_edit: "Edits files freely · asks before running commands",
    full_auto: "Works without asking",
  }[task.effectiveMode];

  const from = {
    agent: task.agentName ? `set by the ${task.agentName} agent` : "set by its agent",
    card: "set on this card",
    default: "your default for new work",
  }[task.permissionSource];

  const asks = task.effectiveMode !== "full_auto";

  return (
    <div className="mt-3 flex items-baseline gap-2 text-[11px]">
      <span className="font-semibold uppercase tracking-wide text-fg-muted">
        Permission
      </span>
      <span className={asks ? "text-fg" : "text-tier-easy"}>{says}</span>
      {/* Naming the source is the whole point — it turns "why is this asking me"
          into a place to go and change it. */}
      <span className="text-fg-muted">· {from}</span>
    </div>
  );
}

/**
 * Where this card sits in an epic — either as the epic, or as one of its parts.
 *
 * Both directions are shown from the same panel because they are the same
 * question asked from two ends. Reading a sub-ticket, "what is this part of" is
 * the missing context; reading an epic, "what is left" is.
 */
function EpicPanel({
  task,
  boardTasks,
  onOpenTask,
}: {
  task: Task;
  boardTasks: Task[];
  onOpenTask?: (t: Task) => void;
}) {
  const parent = task.parentId
    ? boardTasks.find((t) => t.id === task.parentId)
    : undefined;
  const children = boardTasks.filter((t) => t.parentId === task.id);
  if (!parent && children.length === 0) return null;

  return (
    <div className="border-b border-border px-5 py-3">
      {parent && (
        <button
          onClick={() => onOpenTask?.(parent)}
          disabled={!onOpenTask}
          className="text-[11px] text-fg-muted hover:text-accent-fg disabled:hover:text-fg-muted"
        >
          ↳ part of <span className="font-medium">{parent.title}</span>
        </button>
      )}
      {children.length > 0 && (
        <>
          <div className="mb-1.5 text-[11px] font-semibold uppercase tracking-wide text-fg-muted">
            Sub-tasks · {children.filter((c) => resolved(c)).length} of {children.length} done
          </div>
          <div className="space-y-1">
            {children.map((child) => (
              <button
                key={child.id}
                onClick={() => onOpenTask?.(child)}
                disabled={!onOpenTask}
                className="flex w-full items-center gap-2 rounded-lg border border-border bg-panel-2 px-2.5 py-1.5 text-left text-xs hover:border-accent disabled:hover:border-border"
              >
                <span className="min-w-0 flex-1 truncate">{child.title}</span>
                {child.agentName && (
                  <span className="shrink-0 text-[10px] text-fg-muted">{child.agentName}</span>
                )}
                <span
                  className={`shrink-0 text-[10px] ${
                    child.stepStatus === "failed" ? "text-danger-fg" : "text-fg-muted"
                  }`}
                >
                  {child.stepStatus === "failed" ? "failed" : child.boardColumn}
                </span>
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

/** Reached an end state a person would call finished-with. */
const resolved = (t: Task) => t.boardColumn === "review" || t.boardColumn === "done";

/**
 * The diff, with a comment gutter.
 *
 * Clicking a line opens a note anchored to that file and line. "Ask to fix"
 * turns the note into a scoped run in this task's existing worktree, so the
 * correction lands on the same branch and shows up in this same diff —
 * which is the difference between reviewing work and re-describing it.
 */
function DiffView({
  diff,
  taskId,
  onBack,
  onFixStarted,
}: {
  diff: string;
  taskId: string;
  onBack: () => void;
  onFixStarted: () => void;
}) {
  const lines = useMemo(() => annotateDiff(diff), [diff]);
  const [openAt, setOpenAt] = useState<number | null>(null);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [sent, setSent] = useState<string | null>(null);

  const submit = async (fix: boolean) => {
    if (openAt === null || !note.trim() || busy) return;
    const line = lines[openAt];
    setBusy(true);
    try {
      await api.postComment(taskId, note.trim(), undefined, undefined, {
        file_path: line.file ?? undefined,
        line: line.newLine ?? undefined,
        hunk: hunkText(lines, line.hunk),
        fix,
      });
      setNote("");
      setOpenAt(null);
      setSent(fix ? "Fix queued — it'll appear in this diff." : "Note saved to the card.");
      if (fix) onFixStarted();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div>
      <div className="mb-3 flex items-center justify-between">
        <Button variant="ghost" size="xs" onClick={onBack}>
          ← back to stream
        </Button>
        <span className="text-[11px] text-fg-muted">Click a line to comment on it</span>
      </div>

      {sent && (
        <div className="mb-2 rounded-lg bg-tier-easy-soft px-3 py-2 text-xs text-tier-easy">
          {sent}
        </div>
      )}

      <div className="overflow-x-auto rounded-lg bg-panel-2 py-2 font-mono text-xs leading-relaxed">
        {lines.map((line, i) => (
          <div key={i}>
            <div
              onClick={() => isCommentable(line) && setOpenAt(openAt === i ? null : i)}
              className={`group flex gap-2 px-3 ${
                isCommentable(line) ? "cursor-pointer hover:bg-panel" : ""
              } ${
                line.kind === "add"
                  ? "text-tier-easy"
                  : line.kind === "del"
                    ? "text-danger-fg"
                    : line.kind === "hunk"
                      ? "text-tier-medium"
                      : "text-fg-muted"
              }`}
            >
              <span className="w-8 shrink-0 select-none text-right text-fg-muted/50">
                {line.newLine ?? ""}
              </span>
              <span className="w-3 shrink-0 select-none text-fg-muted opacity-0 group-hover:opacity-100">
                {isCommentable(line) ? "+" : ""}
              </span>
              <span className="whitespace-pre">{line.text || " "}</span>
            </div>

            {openAt === i && (
              <div className="my-1 rounded-lg border border-accent/40 bg-panel p-2.5 font-sans">
                <div className="text-[11px] text-fg-muted">
                  {line.file ?? "this change"}
                  {line.newLine ? ` · line ${line.newLine}` : ""}
                </div>
                <textarea
                  autoFocus
                  value={note}
                  onChange={(e) => setNote(e.target.value)}
                  rows={2}
                  placeholder="What's wrong with this?"
                  className="mt-1.5 w-full resize-none rounded-lg border border-border bg-panel px-2.5 py-1.5 text-sm outline-none focus:border-accent"
                />
                <div className="mt-2 flex flex-wrap gap-2">
                  <Button variant="primary" size="sm" onClick={() => submit(true)} disabled={busy || !note.trim()}>
                    {busy ? "…" : "Ask to fix"}
                  </Button>
                  <Button variant="secondary" size="sm" onClick={() => submit(false)} disabled={busy || !note.trim()}>
                    Just comment
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    onClick={() => {
                      setOpenAt(null);
                      setNote("");
                    }}
                  >
                    Cancel
                  </Button>
                </div>
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

/**
 * What the card asks for — the text that becomes the agent's brief — and the
 * place to change it before the next run. First in the drawer's scroll,
 * because "what is this card?" is the question the drawer opens to answer.
 *
 * Editing is refused while a run is live, the same rule as reassignment:
 * rewriting the brief mid-run would leave the agent working from words the
 * card no longer says. The server enforces it; the disabled button just says
 * so up front.
 *
 * Clamped at eight lines with its own toggle: imported GitHub issues and
 * epic briefs run long, and the description must not push the transcript
 * off the screen by default.
 */
function Description({
  task,
  running,
  onChanged,
}: {
  task: Task;
  running: boolean;
  onChanged: () => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const [draft, setDraft] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const text = task.prompt;
  // Cheap length heuristic rather than measuring: the toggle appearing on a
  // borderline description is harmless, the reverse hides content.
  const long = text.length > 420 || text.split("\n").length > 8;

  const save = async () => {
    if (draft === null || saving) return;
    setSaving(true);
    setError(null);
    try {
      await api.moveTask(task.id, { prompt: draft });
      setDraft(null);
      onChanged();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setSaving(false);
    }
  };

  if (!text.trim() && draft === null) return null;

  return (
    <div className="border-b border-border px-5 py-3">
      <div className="mb-1.5 flex items-center justify-between">
        <span className="text-[11px] font-semibold uppercase tracking-wide text-fg-muted">
          Description
        </span>
        {draft === null && (
          // The kit button takes no pointer events while disabled, so the
          // reason it is disabled rides on a wrapper that still shows it.
          <span
            title={
              running
                ? "The agent is working from this brief — cancel the run to rewrite it"
                : "Edit the card's brief; the next run uses the new text"
            }
          >
            <Button variant="ghost" size="xs" onClick={() => setDraft(text)} disabled={running}>
              Edit
            </Button>
          </span>
        )}
      </div>

      {draft === null ? (
        <>
          <div
            className={`whitespace-pre-wrap text-[13px] leading-relaxed ${
              long && !expanded ? "line-clamp-[8]" : ""
            }`}
          >
            {text}
          </div>
          {long && (
            <Button variant="link" size="xs" onClick={() => setExpanded(!expanded)} className="mt-1">
              {expanded ? "Show less" : "Show more"}
            </Button>
          )}
        </>
      ) : (
        <>
          <textarea
            autoFocus
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            rows={Math.min(14, Math.max(4, draft.split("\n").length + 1))}
            className="w-full resize-y rounded-lg border border-accent bg-panel px-2.5 py-2 text-[13px] leading-relaxed outline-none"
          />
          <div className="mt-1.5 flex items-center gap-2">
            <Button variant="primary" size="xs" onClick={save} disabled={saving || !draft.trim() || draft === text}>
              {saving ? "Saving…" : "Save"}
            </Button>
            <Button
              variant="secondary"
              size="xs"
              onClick={() => {
                setDraft(null);
                setError(null);
              }}
            >
              Cancel
            </Button>
            {!draft.trim() && (
              <span className="text-[11px] text-danger-fg">the description can't be empty</span>
            )}
          </div>
          {error && (
            <div className="mt-1.5 rounded-lg bg-danger-subtle px-2.5 py-1.5 text-[11px] text-danger-fg">
              {error}
            </div>
          )}
        </>
      )}
    </div>
  );
}

/**
 * Move the card between columns without leaving the drawer — the same PATCH
 * the board's drag lands on, with the same meanings. "In Progress" is not a
 * label change: it starts the agent, exactly like dropping the card there,
 * and the button says so before you click. While a run is live every other
 * move is refused server-side (cancel first), so the segments say that too
 * instead of letting a click bounce off a 409.
 */
function StatusMover({
  task,
  running,
  onChanged,
}: {
  task: Task;
  running: boolean;
  onChanged: () => void;
}) {
  const [moving, setMoving] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [ask, setAsk] = useState<ForecastAsk | null>(null);
  const [estimate, setEstimate] = useState<string | null>(null);
  const waitingFor = unresolvedBlockers(task);
  // What starting it is likely to cost — only worth asking while it can start.
  useEffect(() => {
    if (task.boardColumn !== "backlog") return setEstimate(null);
    api
      .taskEstimate(task.id)
      .then((r) => setEstimate(estimateLine(r.estimate)))
      .catch(() => setEstimate(null));
  }, [task.id, task.boardColumn]);
  const cols: { key: Task["boardColumn"]; label: string; hint: string }[] = [
    { key: "backlog", label: "Backlog", hint: "File it for later" },
    {
      key: "running",
      label: "In Progress",
      hint:
        waitingFor.length > 0
          ? `Blocked by ${waitingFor.map((b) => b.title).join(", ")} — land ${
              waitingFor.length === 1 ? "that card" : "those cards"
            } first`
          : "Starts the agent on this card",
    },
    { key: "review", label: "Review", hint: "Park it for a person to look at" },
    { key: "done", label: "Done", hint: "Mark it finished" },
  ];

  const move = async (col: Task["boardColumn"], acknowledgeForecast = false) => {
    if ((col === task.boardColumn && !acknowledgeForecast) || moving) return;
    setMoving(col);
    setError(null);
    setAsk(null);
    try {
      await api.moveTask(task.id, { board_column: col, acknowledge_forecast: acknowledgeForecast || undefined });
      onChanged();
    } catch (e) {
      // Similar runs say this could overrun a budget: the person decides.
      const question = parseForecastAsk(String(e));
      if (question) setAsk(question);
      else setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setMoving(null);
    }
  };

  return (
    <div className="border-b border-border px-5 py-3">
      <div className="mb-1.5 text-[11px] font-semibold uppercase tracking-wide text-fg-muted">
        Status
      </div>
      <div className="flex w-fit flex-wrap gap-0.5 rounded-xl bg-panel-2 p-0.5">
        {cols.map((c) => {
          const current = task.boardColumn === c.key;
          const blocked =
            (running && !current) || (c.key === "running" && waitingFor.length > 0);
          return (
            <button
              key={c.key}
              onClick={() => move(c.key)}
              disabled={current || blocked || moving !== null}
              title={
                current
                  ? "Where the card is now"
                  : running
                    ? "The agent is still working — cancel the run first"
                    : c.hint
              }
              className={`relative rounded-lg px-2.5 py-1 text-xs transition-colors ${
                current
                  ? "font-semibold text-accent-fg"
                  : "text-fg-muted hover:text-fg disabled:opacity-40"
              }`}
            >
              {/* One pill sliding between segments, like the page tabs — the
                  eye follows the move instead of hunting for the highlight. */}
              {current && (
                <motion.span
                  layoutId="drawer-status-pill"
                  transition={springy}
                  className="absolute inset-0 rounded-lg bg-accent/10 ring-1 ring-accent/30"
                />
              )}
              <span className="relative">{moving === c.key ? "Moving…" : c.label}</span>
            </button>
          );
        })}
      </div>
      {estimate && !ask && <div className="mt-1.5 text-[11px] text-fg-muted">Starting it: {estimate}</div>}
      {ask && (
        <div className="mt-1.5 rounded-lg bg-warning-subtle px-2.5 py-1.5 text-[11px] text-warning-fg">
          {ask.message.charAt(0).toUpperCase() + ask.message.slice(1)}.
          <div className="mt-1.5 flex gap-2">
            <Button variant="primary" size="xs" onClick={() => move("running", true)}>
              Start anyway
            </Button>
            <Button variant="secondary" size="xs" onClick={() => setAsk(null)}>
              Not now
            </Button>
          </div>
        </div>
      )}
      {error && (
        <div className="mt-1.5 rounded-lg bg-danger-subtle px-2.5 py-1.5 text-[11px] text-danger-fg">
          {error}
        </div>
      )}
    </div>
  );
}

/**
 * The card's configure-time fields, folded behind one line once configuring
 * is over. The drawer was a wall: assignee, skill, references, plan-first,
 * engine, model, effort and permission all stood permanently open above the
 * transcript, and the thing people open the drawer for — what the agent is
 * doing — started below the fold. Collapsed, all of that is one summary line
 * that still answers "who, on what model, how hard".
 */
function Setup({
  summary,
  defaultOpen,
  children,
}: {
  summary: string;
  defaultOpen: boolean;
  children: React.ReactNode;
}) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <div className="border-b border-border">
      <button
        onClick={() => setOpen(!open)}
        className="flex w-full items-center gap-2 px-5 py-3 text-left hover:bg-panel-2/50"
      >
        <span className="shrink-0 text-[11px] font-semibold uppercase tracking-wide text-fg-muted">
          Setup
        </span>
        {/* Animated with `animate` on mounted elements rather than
            AnimatePresence: nothing here unmounts, so a section can never be
            stranded mid-exit — the worst case under any interruption is a
            finished state, not a half-open one. */}
        <motion.span
          animate={{ opacity: open ? 0 : 1 }}
          transition={{ duration: 0.15 }}
          className="min-w-0 truncate text-[11px] text-fg-muted/80"
        >
          {summary}
        </motion.span>
        <motion.span
          animate={{ rotate: open ? 90 : 0 }}
          transition={springy}
          className="ml-auto shrink-0 text-fg-muted"
        >
          ›
        </motion.span>
      </button>
      <motion.div
        initial={false}
        animate={{ height: open ? "auto" : 0, opacity: open ? 1 : 0 }}
        transition={{ duration: 0.22, ease: "easeInOut" }}
        className="overflow-hidden"
      >
        <div className="px-5 pb-3">{children}</div>
      </motion.div>
    </div>
  );
}

/**
 * The cards this one waits for. The bar for "resolved" is done — landed —
 * because a dependent run branches from main, and a blocker parked in review
 * has a diff that is not there yet. Shown right under Status because it is
 * the reason In Progress may refuse.
 */
function Blockers({
  task,
  boardTasks,
  onChanged,
  onOpenTask,
}: {
  task: Task;
  boardTasks: Task[];
  onChanged: () => void;
  onOpenTask?: (t: Task) => void;
}) {
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const blockers = task.blockedBy ?? [];

  // Cards this one could still be blocked by: same board, not itself, not
  // already a blocker. The server also refuses cycles; the picker just does
  // not pretend to know the graph better than it.
  const candidates = boardTasks.filter(
    (t) => t.id !== task.id && !blockers.some((b) => b.id === t.id),
  );

  const add = async (blockedBy: string) => {
    setAdding(false);
    setError(null);
    try {
      await api.addTaskBlocker(task.id, blockedBy);
      onChanged();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    }
  };

  const remove = async (blockerId: string) => {
    setError(null);
    try {
      await api.removeTaskBlocker(task.id, blockerId);
      onChanged();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    }
  };

  const setAutoStart = async (on: boolean) => {
    setError(null);
    try {
      await api.moveTask(task.id, { start_when_unblocked: on });
      onChanged();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    }
  };

  if (blockers.length === 0 && candidates.length === 0 && !task.blockedNote) return null;

  return (
    <div className="border-b border-border px-5 py-3">
      <div className="mb-1.5 flex items-center justify-between">
        <span className="text-[11px] font-semibold uppercase tracking-wide text-fg-muted">
          Blocked by
        </span>
        {!adding && candidates.length > 0 && (
          <Button variant="ghost" size="xs" onClick={() => setAdding(true)}>
            + Add
          </Button>
        )}
      </div>

      {task.blockedNote && (
        <div className="mb-2 rounded-lg bg-warning-subtle px-2.5 py-1.5 text-[11px] text-warning-fg">
          <span className="font-medium">The agent reported it is stuck:</span> {task.blockedNote}
        </div>
      )}

      {blockers.length === 0 && !adding && !task.blockedNote && (
        <div className="text-[11px] text-fg-muted/70">
          Nothing — this card can start any time.
        </div>
      )}

      <div className="flex flex-wrap gap-1.5">
        <AnimatePresence initial={false}>
          {blockers.map((b) => {
            const landed = b.boardColumn === "done";
            const full = boardTasks.find((t) => t.id === b.id);
            return (
              <motion.span
                key={b.id}
                layout
                initial={{ opacity: 0, scale: 0.95 }}
                animate={{ opacity: 1, scale: 1 }}
                exit={{ opacity: 0, scale: 0.95 }}
                className={`flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[11px] ${
                  landed
                    ? "bg-tier-easy/10 text-tier-easy"
                    : "bg-warning-subtle text-warning-fg"
                }`}
                title={landed ? "Landed — no longer blocking" : "Not landed yet — still blocking"}
              >
                <span className={`size-1.5 rounded-full ${landed ? "bg-tier-easy" : "bg-warning"}`} />
                {full && onOpenTask ? (
                  <button onClick={() => onOpenTask(full)} className="hover:underline">
                    {b.title}
                  </button>
                ) : (
                  b.title
                )}
                <button
                  onClick={() => remove(b.id)}
                  title="Remove this dependency"
                  className="opacity-60 hover:opacity-100"
                >
                  ×
                </button>
              </motion.span>
            );
          })}
        </AnimatePresence>
      </div>

      {/* Only while something still blocks it: once they have all landed,
          starting is a click away and there is nothing left to wait for. */}
      {blockers.some((b) => b.boardColumn !== "done") && task.boardColumn === "backlog" && (
        <label className="mt-2 flex cursor-pointer items-center gap-2 text-[11px] text-fg-muted">
          <input
            type="checkbox"
            checked={task.startWhenUnblocked}
            onChange={(e) => setAutoStart(e.target.checked)}
            className="accent-accent"
          />
          Start by itself when {blockers.length === 1 ? "it lands" : "they have all landed"}
        </label>
      )}

      {adding && (
        <select
          autoFocus
          defaultValue=""
          onChange={(e) => e.target.value && add(e.target.value)}
          onBlur={() => setAdding(false)}
          className="mt-1.5 w-full rounded-lg border border-border bg-panel px-2 py-1.5 text-xs"
        >
          <option value="" disabled>
            Which card must land first?
          </option>
          {candidates.map((t) => (
            <option key={t.id} value={t.id}>
              {t.title}
            </option>
          ))}
        </select>
      )}

      {error && (
        <div className="mt-1.5 rounded-lg bg-danger-subtle px-2.5 py-1.5 text-[11px] text-danger-fg">
          {error}
        </div>
      )}
    </div>
  );
}
