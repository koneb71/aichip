import { useCallback, useEffect, useMemo, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { ChevronRight, Flag, Plus, Trash2 } from "lucide-react";
import { api, Goal } from "../lib/api";
import { dueLine, flatten, parentChoices, progress } from "../lib/goals";
import { useWorkspace } from "../lib/workspace";
import { Page } from "../components/ui/Surface";
import { EmptyState, PageHeader, Progress } from "../components/ui/Layout";
import { Badge, type Tone } from "../components/ui/Badge";
import { Button, buttonClasses } from "../components/ui/Button";
import { Field, Input, Select, Textarea } from "../components/ui/Field";
import { Dialog } from "../components/ui/Dialog";
import { toast } from "../components/ui/Toast";
import { cn } from "../components/ui/cn";

const STATUS_TONE: Record<Goal["status"], Tone> = { active: "accent", achieved: "success", abandoned: "neutral" };

/**
 * What the work is for.
 *
 * Every run of a card that serves a goal is told the chain from the top goal
 * down — so this page is not bookkeeping: what is written here is what the
 * agents read when they decide the small things.
 */
export default function GoalsPage() {
  const { active } = useWorkspace();
  const [params, setParams] = useSearchParams();
  const [goals, setGoals] = useState<Goal[] | null>(null);
  const [loads, setLoads] = useState(0);
  const [creating, setCreating] = useState<{ parentId: string | null } | null>(null);
  const selected = params.get("goal");
  const select = (id: string | null) => setParams(id ? { goal: id } : {}, { replace: true });

  useEffect(() => {
    if (!active) return;
    let stale = false;
    api
      .goals(active.id)
      .then((r) => !stale && setGoals(r.goals))
      .catch(() => !stale && setGoals([]));
    return () => {
      stale = true;
    };
  }, [active, loads]);
  const reload = useCallback(() => setLoads((n) => n + 1), []);
  const rows = useMemo(() => flatten(goals ?? []), [goals]);

  return (
    <Page wide>
      <PageHeader
        title="Goals"
        icon={<Flag className="size-4" />}
        description="What the work is for. A card that serves a goal tells every one of its runs why — the chain from the top goal down — so agents make the small choices the way you would."
        actions={
          <Button size="sm" variant="primary" icon={<Plus className="size-3.5" />} onClick={() => setCreating({ parentId: null })}>
            New goal
          </Button>
        }
      />
      {goals === null ? (
        <div className="skeleton h-64 rounded-xl" />
      ) : goals.length === 0 ? (
        <EmptyState
          title="No goals yet"
          hint="Start with the one thing this workspace is for. Goals under it come next; cards point at whichever they serve."
        />
      ) : (
        <div className="grid gap-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.1fr)]">
          <ul className="overflow-hidden rounded-xl border border-border bg-panel" aria-label="Goals">
            {rows.map(({ goal, depth }, i) => {
              const pct = progress(goal);
              const due = dueLine(goal.targetDate);
              return (
                <li key={goal.id} className={cn(i > 0 && "border-t border-border")}>
                  <button
                    onClick={() => select(goal.id)}
                    className={cn(
                      "ring-focus flex w-full items-center gap-3 px-3 py-2.5 text-left transition-colors hover:bg-panel-2",
                      selected === goal.id && "bg-accent-subtle",
                    )}
                    style={{ paddingLeft: 12 + depth * 18 }}
                  >
                    {depth > 0 && <ChevronRight className="size-3 shrink-0 text-fg-subtle" aria-hidden />}
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span className={cn("truncate text-[13px] font-medium", goal.status === "abandoned" ? "text-fg-subtle line-through" : "text-fg")}>
                          {goal.title}
                        </span>
                        {goal.status !== "active" && <Badge tone={STATUS_TONE[goal.status]}>{goal.status}</Badge>}
                      </div>
                      <div className="mt-1 flex items-center gap-2">
                        <div className="w-28 shrink-0">
                          <Progress value={goal.done} max={Math.max(goal.total, 1)} tone={goal.status === "achieved" ? "success" : "accent"} className="h-1" label={`${goal.title} progress`} />
                        </div>
                        <span className="tabular whitespace-nowrap text-[11px] text-fg-muted">
                          {pct === null ? "no cards yet" : `${goal.done}/${goal.total} · ${pct}%`}
                        </span>
                        {due && <span className={cn("whitespace-nowrap text-[11px]", due.includes("overdue") ? "text-danger-fg" : "text-fg-subtle")}>{due}</span>}
                      </div>
                    </div>
                  </button>
                </li>
              );
            })}
          </ul>
          {selected ? (
            <GoalDetail
              key={selected}
              id={selected}
              goals={goals}
              onChanged={reload}
              onDeleted={() => {
                select(null);
                reload();
              }}
              onAddChild={() => setCreating({ parentId: selected })}
            />
          ) : (
            <div className="hidden rounded-xl border border-dashed border-border p-8 text-center text-sm text-fg-muted lg:block">
              Pick a goal to see the cards that serve it.
            </div>
          )}
        </div>
      )}
      {creating && active && (
        <NewGoal
          workspaceId={active.id}
          parentId={creating.parentId}
          goals={goals ?? []}
          onClose={() => setCreating(null)}
          onCreated={(id) => {
            setCreating(null);
            reload();
            select(id);
          }}
        />
      )}
    </Page>
  );
}

function NewGoal({
  workspaceId,
  parentId,
  goals,
  onClose,
  onCreated,
}: {
  workspaceId: string;
  parentId: string | null;
  goals: Goal[];
  onClose: () => void;
  onCreated: (id: string) => void;
}) {
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [parent, setParent] = useState(parentId ?? "");
  const [target, setTarget] = useState("");
  const [busy, setBusy] = useState(false);
  const save = async () => {
    setBusy(true);
    try {
      const { id } = await api.createGoal(workspaceId, {
        title,
        description,
        parentId: parent || null,
        targetDate: target || null,
      });
      onCreated(id);
    } catch (e) {
      toast("Not created", { tone: "danger", body: String(e).replace(/^Error:\s*/, "") });
    } finally {
      setBusy(false);
    }
  };
  return (
    <Dialog open onOpenChange={(o) => !o && onClose()} title="New goal" description="Agents read the title and description of every goal above a card's own." width={520}>
      <div className="space-y-3">
        <Field label="Title">{(id) => <Input id={id} autoFocus value={title} maxLength={200} onChange={(e) => setTitle(e.target.value)} placeholder="e.g. Self-serve onboarding" />}</Field>
        <Field label="Why it matters" hint="A sentence or two. This is what an agent is told.">
          {(id) => <Textarea id={id} value={description} maxLength={4000} onChange={(e) => setDescription(e.target.value)} className="min-h-[72px]" />}
        </Field>
        <div className="grid grid-cols-2 gap-3">
          <Field label="Under">
            {(id) => (
              <Select id={id} value={parent} onChange={(e) => setParent(e.target.value)}>
                <option value="">Nothing — a top goal</option>
                {goals.map((g) => (
                  <option key={g.id} value={g.id}>
                    {g.title}
                  </option>
                ))}
              </Select>
            )}
          </Field>
          <Field label="Target date">{(id) => <Input id={id} type="date" value={target} onChange={(e) => setTarget(e.target.value)} />}</Field>
        </div>
        <div className="flex justify-end gap-2 pt-1">
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" loading={busy} disabled={!title.trim()} onClick={() => void save()}>
            Create goal
          </Button>
        </div>
      </div>
    </Dialog>
  );
}

type Detail = Awaited<ReturnType<typeof api.goal>>;

function GoalDetail({
  id,
  goals,
  onChanged,
  onDeleted,
  onAddChild,
}: {
  id: string;
  goals: Goal[];
  onChanged: () => void;
  onDeleted: () => void;
  onAddChild: () => void;
}) {
  const [d, setD] = useState<Detail | null>(null);
  const [draft, setDraft] = useState<{ title: string; description: string; targetDate: string; parentId: string } | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);

  useEffect(() => {
    let stale = false;
    api
      .goal(id)
      .then((r) => {
        if (stale) return;
        setD(r);
        setDraft({ title: r.goal.title, description: r.goal.description, targetDate: r.goal.targetDate ?? "", parentId: r.goal.parentId ?? "" });
      })
      .catch(() => !stale && setD(null));
    return () => {
      stale = true;
    };
  }, [id, goals]);

  if (!d || !draft) return <div className="skeleton h-64 rounded-xl" />;
  const g = d.goal;
  const dirty =
    draft.title !== g.title || draft.description !== g.description || draft.targetDate !== (g.targetDate ?? "") || draft.parentId !== (g.parentId ?? "");

  const save = async (patch: Parameters<typeof api.updateGoal>[1]) => {
    try {
      await api.updateGoal(id, patch);
      onChanged();
    } catch (e) {
      toast("Not saved", { tone: "danger", body: String(e).replace(/^Error:\s*/, "") });
    }
  };

  return (
    <section className="rounded-xl border border-border bg-panel p-4" aria-label="Goal">
      {d.chain.length > 1 && <div className="mb-2 truncate text-[11px] text-fg-subtle">{d.chain.slice(0, -1).join(" › ")}</div>}
      <div className="space-y-3">
        <Input aria-label="Title" value={draft.title} onChange={(e) => setDraft({ ...draft, title: e.target.value })} className="text-[15px] font-semibold" />
        <Textarea
          aria-label="Why it matters"
          value={draft.description}
          placeholder="Why this matters — agents working on its cards read this."
          onChange={(e) => setDraft({ ...draft, description: e.target.value })}
          className="min-h-[72px]"
        />
        <div className="grid grid-cols-2 gap-3">
          <Field label="Under">
            {(fid) => (
              <Select id={fid} value={draft.parentId} onChange={(e) => setDraft({ ...draft, parentId: e.target.value })}>
                <option value="">Nothing — a top goal</option>
                {parentChoices(goals, id).map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.title}
                  </option>
                ))}
              </Select>
            )}
          </Field>
          <Field label="Target date">{(fid) => <Input id={fid} type="date" value={draft.targetDate} onChange={(e) => setDraft({ ...draft, targetDate: e.target.value })} />}</Field>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Select className="w-36" aria-label="Status" value={g.status} onChange={(e) => void save({ status: e.target.value as Goal["status"] })}>
            <option value="active">Active</option>
            <option value="achieved">Achieved</option>
            <option value="abandoned">Abandoned</option>
          </Select>
          <Button size="sm" variant="ghost" icon={<Plus className="size-3.5" />} onClick={onAddChild}>
            Goal under this
          </Button>
          <span className="ml-auto" />
          <Button size="sm" variant="ghost" icon={<Trash2 className="size-3.5" />} onClick={() => setConfirmDelete(true)}>
            Delete
          </Button>
          <Button
            size="sm"
            variant="primary"
            disabled={!dirty || !draft.title.trim()}
            onClick={() =>
              void save({
                title: draft.title,
                description: draft.description,
                targetDate: draft.targetDate || null,
                parentId: draft.parentId || null,
              })
            }
          >
            Save
          </Button>
        </div>
      </div>

      <div className="mt-4 grid grid-cols-3 gap-2 border-t border-border pt-3 text-center">
        <Stat label="Cards done" value={`${g.done}/${g.total}`} />
        <Stat label="Runs" value={String(d.runs)} />
        <Stat label="Spent" value={`$${d.spendUsd.toFixed(2)}`} />
      </div>

      <h3 className="mt-4 text-xs font-semibold text-fg">Cards serving it</h3>
      {d.cards.length === 0 ? (
        <p className="mt-1 text-xs text-fg-muted">None yet. Pick this goal on a card — or give it to a manager, whose cards then serve it.</p>
      ) : (
        <ul className="mt-1.5 divide-y divide-border rounded-md border border-border">
          {d.cards.map((c) => (
            <li key={c.id} className="flex items-center gap-2 px-2.5 py-1.5 text-xs">
              <Badge tone={c.column === "done" ? "success" : c.column === "running" ? "accent" : "neutral"}>{c.column}</Badge>
              <Link to={`/projects/${c.projectId}?task=${c.id}`} className="min-w-0 flex-1 truncate text-fg hover:underline">
                {c.title}
              </Link>
              {!c.direct && <span className="text-[11px] text-fg-subtle">via a goal below</span>}
              {c.agent && <span className="text-fg-muted">{c.agent}</span>}
            </li>
          ))}
        </ul>
      )}
      {d.projects.length > 0 && (
        <div className="mt-3 flex flex-wrap items-center gap-1.5 text-xs text-fg-muted">
          In
          {d.projects.map((p) => (
            <Link key={p.id} to={`/projects/${p.id}`} className={buttonClasses({ size: "xs", variant: "secondary" })}>
              {p.name}
            </Link>
          ))}
        </div>
      )}

      <Dialog
        open={confirmDelete}
        onOpenChange={setConfirmDelete}
        title="Delete this goal?"
        description="Goals under it move up a level, and its cards then serve the goal above it. Nothing else is deleted."
        width={420}
      >
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={() => setConfirmDelete(false)}>
            Cancel
          </Button>
          <Button
            variant="danger"
            onClick={async () => {
              await api.deleteGoal(id).catch(() => {});
              setConfirmDelete(false);
              onDeleted();
            }}
          >
            Delete goal
          </Button>
        </div>
      </Dialog>
    </section>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <div className="tabular text-[15px] font-semibold text-fg">{value}</div>
      <div className="text-[11px] text-fg-muted">{label}</div>
    </div>
  );
}
