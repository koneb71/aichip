import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { api, OrgAssignment, OrgMember, OrgMessage, OrgRunDetail } from "../../lib/api";
import { isTerminal, isWorking, needsYou, statusColor, statusLabel } from "../../lib/runStatus";
import { NARROW, useMediaQuery } from "../../lib/useMediaQuery";
import { Markdown } from "../Markdown";
import { ActivityLine, RunStream } from "../RunStream";
import { StreamEvent, useRunStream } from "../../lib/ws";
import { PlanReview } from "./PlanReview";

/** What a teammate is doing right now, derived from their assignments. */
type MemberState = "idle" | "working" | "asking" | "done" | "blocked";

export function OrgRunView({ runId, onClose }: { runId: string; onClose: () => void }) {
  const [run, setRun] = useState<OrgRunDetail | null>(null);
  // The same transcript a solo task shows, but attributed per teammate via
  // each event's step id. Without this a team run could only ever say
  // "working" — you could watch the conversation but never the work.
  const events = useRunStream(runId);
  const [pane, setPane] = useState<Pane>("chat");
  const narrow = useMediaQuery(NARROW);
  const feedRef = useRef<HTMLDivElement>(null);
  const atBottom = useRef(true);

  const refresh = useCallback(async () => {
    try {
      setRun(await api.orgRun(runId));
    } catch {
      /* transient */
    }
  }, [runId]);

  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, 1500);
    return () => clearInterval(interval);
  }, [refresh]);

  // Follow the conversation unless the user has scrolled up to read.
  useEffect(() => {
    if (atBottom.current) {
      feedRef.current?.scrollTo({
        top: feedRef.current.scrollHeight,
        behavior: "smooth",
      });
    }
  }, [run?.messages.length]);

  const states = useMemo(() => memberStates(run), [run]);
  // Narrow on purpose: a parked run is not "working", so the typing
  // indicator must not fire while it waits on the user.
  const live = isWorking(run?.status);
  const needsReview = run?.status === "awaiting_approval";

  // A plan waiting on approval is the whole point of having the modal open, so
  // it wins the pane on a narrow screen rather than hiding behind a tab.
  useEffect(() => {
    if (needsReview) setPane("tasks");
  }, [needsReview]);

  // Wide viewports render all three; narrow ones render only the active pane,
  // so the two idle scroll containers aren't kept alive off-screen.
  const show = (id: Pane) => !narrow || pane === id;

  if (!run) {
    return (
      <Shell onClose={onClose} title="Loading…">
        <div className="p-8 text-sm text-ink-dim">Fetching the team…</div>
      </Shell>
    );
  }

  return (
    <Shell
      onClose={onClose}
      title={run.teamName}
      subtitle={run.goal ?? undefined}
      status={run.status}
      cost={run.costUsd}
    >
      {narrow && <PaneTabs pane={pane} onPick={setPane} needsReview={needsReview} />}
      {/* Three fixed columns only once there is room for three. `minmax(0,1fr)`
       *  rather than `1fr`: a bare 1fr floors at min-content, so one long
       *  unbreakable token in the transcript widens the middle column and
       *  pushes Assignments out of the modal's clipped edge. */}
      <div className="flex min-h-0 flex-1 lg:grid lg:grid-cols-[minmax(200px,240px)_minmax(0,1fr)_minmax(0,300px)]">
        {/* ── Roster ───────────────────────────────────────────── */}
        {show("team") && (
          <div className="min-h-0 min-w-0 flex-1 overflow-y-auto border-line p-3 lg:flex-none lg:border-r">
            {!narrow && <SectionLabel>Team</SectionLabel>}
            <div className="mt-2 flex flex-col gap-2">
              {run.roster.map((member, i) => (
                <MemberCard
                  key={member.name}
                  member={member}
                  state={states[member.name] ?? "idle"}
                  index={i}
                />
              ))}
            </div>
          </div>
        )}

        {/* ── Conversation ─────────────────────────────────────── */}
        {show("chat") && (
          <div className="flex min-h-0 min-w-0 flex-1 flex-col lg:flex-none">
            <div ref={feedRef} className="min-h-0 min-w-0 flex-1 overflow-y-auto px-4 py-4 sm:px-5"
              onScroll={(e) => {
                const el = e.currentTarget;
                atBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 60;
              }}
            >
              <div className="flex min-w-0 flex-col gap-3">
                <AnimatePresence initial={false}>
                  {run.messages.map((message) => (
                    <Message
                      key={message.id}
                      message={message}
                      color={colorOf(run.roster, message.from)}
                    />
                  ))}
                </AnimatePresence>
                {live && <WorkingIndicator states={states} roster={run.roster} />}
              </div>
            </div>
            {run.error && (
              <div className="mx-4 mb-3 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger sm:mx-5">
                {run.error}
              </div>
            )}
          </div>
        )}

        {/* ── Assignments ──────────────────────────────────────── */}
        {show("tasks") && (
          <div className="min-h-0 min-w-0 flex-1 overflow-y-auto border-line p-3 lg:flex-none lg:border-l">
            {!narrow && <SectionLabel>Assignments</SectionLabel>}
            <div className="mt-2 flex flex-col gap-2">
              {needsReview ? (
                <PlanReview run={run} onChanged={refresh} />
              ) : (
                <>
                  <AnimatePresence initial={false}>
                    {run.assignments
                      .filter((a) => a.kind === "assignment")
                      .map((a) => (
                        <AssignmentCard
                          key={a.id}
                          assignment={a}
                          color={colorOf(run.roster, a.assignee ?? "")}
                          runEnded={isTerminal(run.status)}
                          events={events}
                        />
                      ))}
                  </AnimatePresence>
                  {run.assignments.every((a) => a.kind === "manager") && (
                    <div className="rounded-xl border border-dashed border-line p-4 text-center text-xs text-ink-dim">
                      The manager is still working out the plan…
                    </div>
                  )}
                </>
              )}
            </div>
          </div>
        )}
      </div>
    </Shell>
  );
}

type Pane = "team" | "chat" | "tasks";

/** Pane switcher for narrow viewports, where the three columns are stacked one
 *  at a time instead of side by side. */
function PaneTabs({
  pane,
  onPick,
  needsReview,
}: {
  pane: Pane;
  onPick: (pane: Pane) => void;
  needsReview: boolean;
}) {
  const tabs: [Pane, string][] = [
    ["team", "Team"],
    ["chat", "Conversation"],
    ["tasks", needsReview ? "Approve plan" : "Assignments"],
  ];
  return (
    <div className="flex shrink-0 gap-1 border-b border-line px-2 py-1.5">
      {tabs.map(([id, label]) => (
        <button
          key={id}
          onClick={() => onPick(id)}
          aria-current={pane === id}
          className={`flex-1 rounded-lg px-2 py-1.5 text-xs font-medium ${
            pane === id ? "bg-panel-2 text-ink" : "text-ink-dim hover:text-ink"
          }`}
        >
          {label}
          {id === "tasks" && needsReview && (
            <span className="ml-1 inline-block h-1.5 w-1.5 rounded-full bg-[#d97706] align-middle" />
          )}
        </button>
      ))}
    </div>
  );
}

function Shell({
  title,
  subtitle,
  status,
  cost,
  onClose,
  children,
}: {
  title: string;
  subtitle?: string;
  status?: string;
  cost?: number | null;
  onClose: () => void;
  children: React.ReactNode;
}) {
  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 p-0 sm:p-4 lg:p-6"
      onClick={onClose}
    >
      <motion.div
        initial={{ y: 24, scale: 0.98 }}
        animate={{ y: 0, scale: 1 }}
        exit={{ y: 24, scale: 0.98 }}
        transition={{ type: "spring", stiffness: 360, damping: 32 }}
        onClick={(e) => e.stopPropagation()}
        className="card-shadow flex h-full w-full max-w-6xl flex-col overflow-hidden border-line bg-panel sm:h-[88vh] sm:rounded-2xl sm:border"
      >
        <header className="flex items-start gap-3 border-b border-line px-4 py-3 sm:px-5">
          <div className="min-w-0 flex-1">
            {/* Wraps rather than shoving the ✕ off the edge: a long team name
             *  plus a status chip plus a cost overflows a phone header. */}
            <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
              <span className="truncate text-base font-semibold">{title}</span>
              {status && <StatusChip status={status} />}
              {cost != null && (
                <span className="text-xs text-ink-dim">${cost.toFixed(3)}</span>
              )}
            </div>
            {subtitle && (
              <div className="mt-0.5 line-clamp-1 text-xs text-ink-dim">{subtitle}</div>
            )}
          </div>
          <button onClick={onClose} className="text-ink-dim hover:text-ink">
            ✕
          </button>
        </header>
        {children}
      </motion.div>
    </motion.div>
  );
}

function SectionLabel({ children }: { children: React.ReactNode }) {
  return (
    <div className="px-1 text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
      {children}
    </div>
  );
}

function MemberCard({
  member,
  state,
  index,
}: {
  member: OrgMember;
  state: MemberState;
  index: number;
}) {
  const busy = state === "working" || state === "asking";
  return (
    <motion.div
      initial={{ opacity: 0, x: -12 }}
      animate={{ opacity: 1, x: 0 }}
      transition={{ delay: index * 0.05 }}
      className="relative rounded-xl border bg-panel p-2.5"
      style={{
        borderColor: busy ? member.color : "var(--color-line)",
        boxShadow: busy ? `0 0 0 3px ${member.color}1a` : undefined,
      }}
    >
      <div className="flex items-center gap-2.5">
        <div className="relative">
          <motion.span
            className="flex h-8 w-8 items-center justify-center rounded-lg text-sm font-bold text-white"
            style={{ background: member.color }}
            animate={busy ? { scale: [1, 1.06, 1] } : { scale: 1 }}
            transition={busy ? { repeat: Infinity, duration: 1.8 } : undefined}
          >
            {member.name.slice(0, 1).toUpperCase()}
          </motion.span>
          {busy && (
            <motion.span
              className="absolute inset-0 rounded-lg"
              style={{ border: `2px solid ${member.color}` }}
              animate={{ scale: [1, 1.5], opacity: [0.6, 0] }}
              transition={{ repeat: Infinity, duration: 1.8 }}
            />
          )}
          {state === "done" && (
            <motion.span
              initial={{ scale: 0 }}
              animate={{ scale: 1 }}
              className="absolute -bottom-1 -right-1 flex h-4 w-4 items-center justify-center rounded-full bg-tier-easy text-[9px] text-white"
            >
              ✓
            </motion.span>
          )}
        </div>
        <div className="min-w-0 flex-1">
          <div className="truncate text-sm font-medium">{member.name}</div>
          <div className="truncate text-[11px] text-ink-dim">
            {member.isManager ? "Manager" : member.title}
          </div>
        </div>
      </div>
      <motion.div layout className="mt-1.5 text-[11px]" style={{ color: busy ? member.color : "var(--color-ink-dim)" }}>
        {stateLabel(state)}
      </motion.div>
    </motion.div>
  );
}

function Message({ message, color }: { message: OrgMessage; color: string }) {
  if (message.kind === "status") {
    return (
      <motion.div
        layout
        initial={{ opacity: 0, scale: 0.96 }}
        animate={{ opacity: 1, scale: 1 }}
        className="self-center rounded-full bg-panel-2 px-3 py-1 text-[11px] text-ink-dim"
      >
        {message.content}
      </motion.div>
    );
  }

  const label =
    message.kind === "assignment"
      ? `assigned to ${message.to}`
      : message.kind === "question"
        ? "asked the manager"
        : message.kind === "answer"
          ? `answered ${message.to}`
          : message.kind === "result"
            ? "reported back"
            : null;

  const accent =
    message.kind === "question"
      ? "var(--color-tier-complex)"
      : message.kind === "answer"
        ? "var(--color-tier-medium)"
        : color;

  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ type: "spring", stiffness: 400, damping: 32 }}
      className="flex gap-2.5"
    >
      <span
        className="mt-0.5 flex h-7 w-7 shrink-0 items-center justify-center rounded-lg text-xs font-bold text-white"
        style={{ background: color }}
      >
        {message.from.slice(0, 1).toUpperCase()}
      </span>
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2">
          <span className="text-sm font-medium">{message.from}</span>
          {label && (
            <span
              className="rounded-full px-1.5 py-0.5 text-[10px]"
              style={{ background: `${accent}1a`, color: accent }}
            >
              {label}
            </span>
          )}
          <span className="text-[10px] text-ink-dim">
            {new Date(message.ts).toLocaleTimeString([], {
              hour: "2-digit",
              minute: "2-digit",
            })}
          </span>
        </div>
        <div
          className="mt-1 rounded-xl rounded-tl-sm px-3 py-2 text-sm"
          style={{
            background: message.kind === "question" ? "var(--color-tier-complex-soft)" : "var(--color-panel-2)",
          }}
        >
          <Markdown>{message.content}</Markdown>
        </div>
      </div>
    </motion.div>
  );
}

function WorkingIndicator({
  states,
  roster,
}: {
  states: Record<string, MemberState>;
  roster: OrgMember[];
}) {
  const busy = roster.filter((m) => {
    const s = states[m.name];
    return s === "working" || s === "asking";
  });
  if (busy.length === 0) return null;
  return (
    <motion.div layout initial={{ opacity: 0 }} animate={{ opacity: 1 }} className="flex items-center gap-2">
      <div className="flex -space-x-1.5">
        {busy.map((m) => (
          <span
            key={m.name}
            className="flex h-5 w-5 items-center justify-center rounded-full border-2 border-panel text-[9px] font-bold text-white"
            style={{ background: m.color }}
          >
            {m.name.slice(0, 1).toUpperCase()}
          </span>
        ))}
      </div>
      <div className="flex gap-1 rounded-full bg-panel-2 px-2.5 py-1.5">
        {[0, 1, 2].map((i) => (
          <motion.span
            key={i}
            className="h-1.5 w-1.5 rounded-full bg-ink-dim"
            animate={{ opacity: [0.3, 1, 0.3] }}
            transition={{ repeat: Infinity, duration: 1.2, delay: i * 0.2 }}
          />
        ))}
      </div>
      <span className="text-[11px] text-ink-dim">
        {busy.map((m) => m.name).join(", ")} {busy.length === 1 ? "is" : "are"} working…
      </span>
    </motion.div>
  );
}

function AssignmentCard({
  assignment,
  color,
  runEnded,
  events,
}: {
  assignment: OrgAssignment;
  color: string;
  runEnded: boolean;
  events: StreamEvent[];
}) {
  const [open, setOpen] = useState(false);
  const running = isWorking(assignment.status) && !runEnded;
  return (
    <motion.button
      layout
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      onClick={() => setOpen((o) => !o)}
      className="rounded-xl border bg-panel p-2.5 text-left"
      style={{
        borderColor: running ? color : "var(--color-line)",
        boxShadow: running ? `0 0 0 3px ${color}14` : undefined,
      }}
    >
      <div className="flex items-start gap-2">
        <StatusPip status={assignment.status} color={color} live={running} />
        <div className="min-w-0 flex-1">
          <div className="text-xs font-medium leading-snug">
            {assignment.title ?? assignment.key}
          </div>
          {assignment.assignee && (
            <div className="mt-0.5 text-[11px]" style={{ color }}>
              {assignment.assignee}
            </div>
          )}
          {/* Collapsed, each teammate still shows their current action. */}
          <ActivityLine events={events} stepId={assignment.id} live={running} className="mt-1" />
        </div>
      </div>
      <AnimatePresence>
        {open && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            className="overflow-hidden"
          >
            <div className="mt-2 border-t border-line pt-2 text-[11px] text-ink-dim">
              {assignment.touches.length > 0 && (
                <div className="mb-1.5 flex flex-wrap gap-1">
                  {assignment.touches.map((path) => (
                    <span
                      key={path}
                      title="Declared file scope — assignments with no overlap run at the same time"
                      className="rounded bg-panel-2 px-1.5 py-0.5 font-mono text-[10px]"
                    >
                      {path}
                    </span>
                  ))}
                </div>
              )}
              {assignment.output || assignment.brief || "No detail yet."}
            </div>
            {/* Expanded, the full transcript for this teammate alone — the
                same renderer a solo task uses, filtered to their step. */}
            <div className="mt-2 max-h-72 overflow-y-auto border-t border-line pt-2">
              <RunStream
                events={events}
                stepId={assignment.id}
                empty="No activity recorded for this assignment yet."
              />
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </motion.button>
  );
}

function StatusPip({ status, color, live }: { status: string; color: string; live: boolean }) {
  const fill =
    status === "completed"
      ? "var(--color-tier-easy)"
      : status === "failed"
        ? "var(--color-danger)"
        : status === "canceled"
          ? "var(--color-ink-dim)"
          : live
            ? color
            : "var(--color-line)";
  return live ? (
    <motion.span
      className="mt-1 h-2 w-2 shrink-0 rounded-full"
      style={{ background: fill }}
      animate={{ opacity: [1, 0.3, 1] }}
      transition={{ repeat: Infinity, duration: 1.4 }}
    />
  ) : (
    <span className="mt-1 h-2 w-2 shrink-0 rounded-full" style={{ background: fill }} />
  );
}

function StatusChip({ status }: { status: string }) {
  const live = isWorking(status);
  const color = statusColor(status);
  return (
    <span
      className="flex items-center gap-1.5 rounded-full px-2 py-0.5 text-[11px]"
      style={{ background: `${color}1a`, color }}
    >
      {live ? (
        <motion.span
          className="h-1.5 w-1.5 rounded-full"
          style={{ background: color }}
          animate={{ opacity: [1, 0.3, 1] }}
          transition={{ repeat: Infinity, duration: 1.4 }}
        />
      ) : (
        <span className="h-1.5 w-1.5 rounded-full" style={{ background: color }} />
      )}
      {statusLabel(status)}
    </span>
  );
}

function colorOf(roster: OrgMember[], name: string): string {
  return roster.find((m) => m.name === name)?.color ?? "var(--color-ink-dim)";
}

function stateLabel(state: MemberState): string {
  switch (state) {
    case "working":
      return "working…";
    case "asking":
      return "waiting on an answer";
    case "done":
      return "finished";
    case "blocked":
      return "blocked";
    default:
      return "standing by";
  }
}

/** Derive each teammate's live state from their assignments and the last
 *  thing they said — the backend stores facts, the UI reads the mood. */
function memberStates(run: OrgRunDetail | null): Record<string, MemberState> {
  if (!run) return {};
  const states: Record<string, MemberState> = {};
  for (const member of run.roster) states[member.name] = "idle";

  for (const a of run.assignments) {
    if (!a.assignee) continue;
    if (isWorking(a.status)) states[a.assignee] = "working";
    else if (a.status === "failed") states[a.assignee] = "blocked";
    else if (a.status === "skipped") continue;
    else if (a.status === "completed" && states[a.assignee] !== "working")
      states[a.assignee] = "done";
  }

  // An unanswered question outranks "working": they're stuck waiting.
  const lastQuestion = [...run.messages].reverse().find((m) => m.kind === "question");
  if (lastQuestion) {
    const answered = run.messages.some(
      (m) => m.kind === "answer" && m.seq > lastQuestion.seq,
    );
    if (!answered && states[lastQuestion.from] === "working") {
      states[lastQuestion.from] = "asking";
    }
  }

  // Nobody is working inside a run that is over. The orchestrator settles step
  // rows now, but runs orphaned before it did still carry 'running' steps, and
  // a pulsing "working…" next to a failed run is a lie either way.
  if (isTerminal(run.status)) {
    const ended = run.status === "completed" ? "done" : "blocked";
    for (const [name, state] of Object.entries(states)) {
      if (state === "working" || state === "asking") states[name] = ended;
    }
  }
  return states;
}
