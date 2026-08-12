import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { motion } from "framer-motion";
import { api, Agent, ChatMessage, ChatSummary, Effort, Skill, Tier } from "../../lib/api";
import { useRunStream } from "../../lib/ws";
import { useAttachments } from "../../lib/useAttachments";
import { agentSpans } from "../../lib/mention";
import { AttachmentBar, AttachmentList } from "../AttachmentBar";
import { ComposerSettings } from "./ComposerSettings";
import { useMentionPicker } from "../MentionPicker";
import { Markdown } from "../Markdown";

/**
 * One conversation: the scroller, the live run, and the composer.
 *
 * Extracted from ChatPanel so the same thread can be the project page's
 * 380px rail and the Chat page's full-width column. It renders a fragment on
 * purpose — the rail's root div and header stay in ChatPanel, so the rail's
 * DOM is exactly what it was before the extraction.
 *
 * Whose state is whose: the thread owns everything about *this conversation*
 * (messages, the live run, the draft, the composer settings). The caller owns
 * which conversation is open and what the list of conversations looks like —
 * which is why title derivation is reported back through `onSent` rather than
 * refreshed here.
 */
export function ChatThread({
  projectId,
  workspaceId,
  chatId,
  chat,
  onSent,
  centered,
}: {
  /** Null for a *general* chat — no project, no repo, no board. Attachments
   *  and the `@` file picker are project machinery and disappear with it. */
  projectId: string | null;
  /**
   * The workspace this *project* belongs to — not whichever one the sidebar is
   * showing. The server resolves `@mentions` against the project's workspace,
   * so resolving them here against any other list would offer agents that
   * cannot bind and draw chips for mentions that did not.
   */
  workspaceId?: string;
  chatId: string | null;
  /** The open chat's summary, for seeding tier/effort. */
  chat?: ChatSummary;
  /** The first message names the chat server-side — the caller's cue to
   *  refresh its list and pick the title up. */
  onSent?: () => void;
  /** Cap the content width for a full-page mount. Off by default, which is
   *  the rail's original layout. */
  centered?: boolean;
}) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [activeRunId, setActiveRunId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  // null = the machine default. Switching mid-chat starts a fresh session,
  // because a session id only means something to the CLI that minted it.
  const [engine, setEngine] = useState<string | null>(null);
  // Seeded from the chat once it loads, then owned here — see the composer.
  const [tier, setTier] = useState<Tier>("medium");
  const [effort, setEffort] = useState<Effort | null>(null);
  // The agent library, fetched once. Both the `@` picker and the message
  // bubbles resolve names against this one list, so a chip is only ever drawn
  // for an agent that exists.
  const [agents, setAgents] = useState<Agent[]>([]);
  // Skills share that `@` namespace, so they are offered from the same picker
  // and drawn as the same chip. Only the enabled ones: a switched-off skill
  // binds to nothing on the server, and offering it would promise otherwise.
  const [skills, setSkills] = useState<Skill[]>([]);
  const scrollRef = useRef<HTMLDivElement>(null);
  const streamEvents = useRunStream(activeRunId);
  const general = projectId === null;
  // Hooks cannot be conditional; the empty id keeps this one inert and the
  // `general` gates below keep its UI out of the tree.
  const att = useAttachments(projectId ?? "");
  const composerRef = useRef<HTMLTextAreaElement>(null);
  // Caret is tracked separately: it moves on click and arrow keys, not just
  // on change, and the mention token depends on where it is.
  const [caret, setCaret] = useState(0);
  const mention = useMentionPicker({
    projectId: projectId ?? "",
    agents,
    skills,
    text: draft,
    caret,
    onApply: (text, nextCaret) => {
      setDraft(text);
      setCaret(nextCaret);
      // The textarea is uncontrolled w.r.t. selection, so place it by hand
      // after React has written the new value.
      requestAnimationFrame(() => {
        composerRef.current?.setSelectionRange(nextCaret, nextCaret);
        composerRef.current?.focus();
      });
    },
  });

  useEffect(() => {
    if (!workspaceId) return;
    setAgents([]); // a stale library would resolve mentions against the wrong workspace
    setSkills([]);
    api.agents(workspaceId).then((r) => setAgents(r.agents)).catch(() => {});
    api
      .skills(workspaceId)
      .then((r) => setSkills(r.skills.filter((s) => s.enabled)))
      .catch(() => {});
  }, [workspaceId]);

  // One list, because the server parses one namespace: a chip is drawn for
  // anything that would actually bind, whichever kind it turns out to be.
  const agentNames = useMemo(
    () => [...agents.map((a) => a.name), ...skills.map((s) => s.name)],
    [agents, skills],
  );

  // Switching conversations must drop the previous thread's messages and
  // stream, or the old run's text bleeds into the new chat. Deliberately an
  // effect rather than a React `key` on this component — a key would also
  // clear the draft and the engine choice, which today's behaviour keeps.
  useEffect(() => {
    setMessages([]);
    setActiveRunId(null);
    setError(null);
  }, [chatId]);

  // A conversation remembers what it was last run with, so reopening it picks
  // up where you left off rather than snapping back to the defaults. Guarded by
  // the ref so a routine refresh of the chat list can't undo a choice you made
  // in the composer but haven't sent yet.
  const seededFor = useRef<string | null>(null);
  useEffect(() => {
    if (!chatId || !chat || seededFor.current === chatId) return;
    seededFor.current = chatId;
    setTier(chat.modelTier ?? "medium");
    setEffort(chat.effort);
  }, [chatId, chat]);

  const refresh = useCallback(async () => {
    if (!chatId) return;
    try {
      const r = await api.chatMessages(chatId);
      setMessages(r.messages);
      setActiveRunId(r.activeRunId);
    } catch {
      /* server restarting; retry next tick */
    }
  }, [chatId]);

  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, 2500);
    return () => clearInterval(interval);
  }, [refresh]);

  useEffect(() => {
    scrollRef.current?.scrollTo({ top: scrollRef.current.scrollHeight });
  }, [messages.length, streamEvents.length]);

  const send = async () => {
    // An attachment on its own is a legitimate turn, so text is not required.
    if (!chatId || activeRunId || att.busy) return;
    if (!draft.trim() && att.ids.length === 0) return;
    setError(null);
    const content = draft.trim();
    const attachmentIds = att.ids;
    // Carry the chips into the optimistic bubble, or they'd vanish for the
    // ~2.5s until the next poll returns the real message.
    const sent = att.items.filter((i) => i.remote).map((i) => i.remote!);
    setDraft("");
    att.clear();
    setMessages((prev) => [
      ...prev,
      {
        id: "pending",
        role: "user",
        content,
        runId: null,
        ts: new Date().toISOString(),
        attachments: sent,
      },
    ]);
    try {
      const r = await api.sendChat(chatId, content, {
        attachmentIds,
        engine: engine ?? undefined,
        modelTier: tier,
        effort,
      });
      setActiveRunId(r.runId);
      // The first message names the chat server-side — the caller's list is
      // what shows it.
      onSent?.();
    } catch (e) {
      setError(String(e));
      refresh();
    }
  };

  // Live stream: show assistant text + tool chips while the turn runs.
  const liveText = streamEvents
    .filter((e) => e.type === "assistant_text")
    .map((e) => String(e.text))
    .join("\n");
  const liveTools = streamEvents.filter((e) => e.type === "tool_call");
  const turnDone = streamEvents.some(
    (e) => e.type === "run_completed" || e.type === "run_failed",
  );

  useEffect(() => {
    if (turnDone) {
      refresh().then(() => setActiveRunId(null));
    }
  }, [turnDone, refresh]);

  // Both wrappers exist only on the Chat page. The rail path renders the
  // children bare, so its DOM is exactly what ChatPanel produced before the
  // extraction — an extra <div>, even an inert one, would already be a
  // different tree to debug against.
  const wrap = (extra: string, children: React.ReactNode) =>
    centered ? <div className={`mx-auto w-full max-w-3xl ${extra}`.trim()}>{children}</div> : children;

  return (
    <>
      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto p-4">
        {wrap(
          "",
          <>
          {messages.length === 0 && !activeRunId && (
            <div className="mt-8 px-4 text-center text-sm text-ink-dim">
              Describe what you want done — e.g. “fix the flaky login test and
              open it for review”. The assistant creates tasks on the board and
              keeps you posted here. Type <span className="font-medium">@</span> to
              hand the work to one of your agents, or to point at a file.
            </div>
          )}
          <div className="flex flex-col gap-3">
            {messages.map((m) => (
              <Message key={m.id + m.ts} message={m} agentNames={agentNames} />
            ))}
            {activeRunId && (
              <div className="flex flex-col gap-2">
                {liveTools.map((t, i) => (
                  <ToolChip key={i} name={String(t.tool_name)} input={t.input} />
                ))}
                {liveText ? (
                  <div className="max-w-[85%] self-start rounded-2xl rounded-bl-sm bg-panel-2 px-3 py-2 text-sm">
                    <Markdown>{liveText}</Markdown>
                  </div>
                ) : (
                  <Thinking />
                )}
              </div>
            )}
          </div>
          </>,
        )}
      </div>

      {error && (
        <div className="mx-4 mb-1 rounded-lg bg-red-50 px-3 py-1.5 text-xs text-danger">
          {error}
        </div>
      )}

      <div className="relative border-t border-line p-3" {...(general ? {} : att.dropProps)}>
        {wrap(
          "relative",
          <>
          {!general && mention.node}
          <div
            className={`flex flex-col gap-1.5 rounded-xl border bg-panel px-3 py-2 focus-within:border-accent ${
              att.dragging ? "border-accent ring-2 ring-accent/30" : "border-line"
            }`}
          >
            {!general && (att.items.length > 0 || att.dragging) && (
              <AttachmentBar
                items={att.items}
                onAdd={att.add}
                onRemove={att.remove}
                full={att.full}
                disabled={!!activeRunId}
              />
            )}
            <div className="flex items-end gap-2">
              {!general && att.items.length === 0 && !att.dragging && (
                <AttachmentBar
                  items={[]}
                  onAdd={att.add}
                  onRemove={att.remove}
                  full={att.full}
                  disabled={!!activeRunId}
                />
              )}
              <textarea
                ref={composerRef}
                value={draft}
                onChange={(e) => {
                  setDraft(e.target.value);
                  setCaret(e.target.selectionStart ?? 0);
                }}
                onSelect={(e) => setCaret(e.currentTarget.selectionStart ?? 0)}
                onPaste={att.onPaste}
                onKeyDown={(e) => {
                  // The picker gets first refusal: otherwise Enter sends the
                  // message instead of choosing the highlighted file.
                  if (!general && mention.handleKey(e)) {
                    e.preventDefault();
                    return;
                  }
                  if (e.key === "Enter" && !e.shiftKey) {
                    e.preventDefault();
                    send();
                  }
                }}
                rows={Math.min(4, Math.max(1, draft.split("\n").length))}
                placeholder={
                  activeRunId ? "Assistant is working…" : "What should we work on?"
                }
                disabled={!!activeRunId}
                className="min-w-0 flex-1 resize-none bg-transparent text-sm outline-none disabled:opacity-60"
              />
              <motion.button
                whileTap={{ scale: 0.92 }}
                onClick={send}
                disabled={
                  !!activeRunId || att.busy || (!draft.trim() && att.ids.length === 0)
                }
                className="rounded-lg bg-accent px-2.5 py-1.5 text-sm text-white disabled:opacity-40"
              >
                ↑
              </motion.button>
            </div>
            {/* Which CLI, which model, and how hard it thinks. All three stick to
                the chat rather than the message — choosing "think harder" and
                having it last one turn would be a strange thing to have chosen. */}
            <ComposerSettings
              engine={engine}
              onEngine={setEngine}
              tier={tier}
              onTier={setTier}
              effort={effort}
              onEffort={setEffort}
              disabled={!!activeRunId}
            />
          </div>
          </>,
        )}
      </div>
    </>
  );
}

function Message({
  message,
  agentNames,
}: {
  message: ChatMessage;
  agentNames: string[];
}) {
  if (message.role === "user") {
    return (
      <motion.div
        initial={{ opacity: 0, y: 4 }}
        animate={{ opacity: 1, y: 0 }}
        className="flex max-w-[85%] flex-col items-end self-end"
      >
        {/* Above the bubble, not inside it: the bubble is solid accent, and
            bordered file chips read badly on it. */}
        <AttachmentList attachments={message.attachments} />
        {message.content && (
          <div className="rounded-2xl rounded-br-sm bg-accent px-3 py-2 text-sm whitespace-pre-wrap text-white">
            <WithMentions text={message.content} agentNames={agentNames} />
          </div>
        )}
      </motion.div>
    );
  }
  if (message.role === "system") {
    return (
      <div className="self-center rounded-full bg-tier-easy-soft px-3 py-1 text-xs text-tier-easy">
        {message.content}
      </div>
    );
  }
  return (
    <motion.div
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      className="max-w-[85%] self-start rounded-2xl rounded-bl-sm bg-panel-2 px-3 py-2 text-sm"
    >
      <Markdown>{message.content}</Markdown>
    </motion.div>
  );
}

/**
 * The message as sent, with `@Name` drawn as a chip.
 *
 * Not decoration: the mention is what decides who does the work, and the only
 * way to tell a mention that bound from a name that merely looks like one is to
 * show the difference. A name with no agent behind it stays plain text —
 * exactly what the server did with it.
 */
function WithMentions({ text, agentNames }: { text: string; agentNames: string[] }) {
  const spans = agentSpans(text, agentNames);
  if (!spans.length) return <>{text}</>;

  const parts: React.ReactNode[] = [];
  let at = 0;
  spans.forEach((span, i) => {
    if (span.start > at) parts.push(text.slice(at, span.start));
    parts.push(
      <span
        key={i}
        className="rounded bg-white/25 px-1 font-medium"
        title={`Assigned to ${span.name}`}
      >
        {text.slice(span.start, span.end)}
      </span>,
    );
    at = span.end;
  });
  if (at < text.length) parts.push(text.slice(at));
  return <>{parts}</>;
}

function ToolChip({ name, input }: { name: string; input: unknown }) {
  const label = (() => {
    const args = (input ?? {}) as Record<string, unknown>;
    if (name === "mcp__aichip__create_task") {
      const who = typeof args.agent_name === "string" ? ` — ${args.agent_name}` : "";
      return `Creating task: ${args.title ?? ""}${who}`;
    }
    if (name === "mcp__aichip__start_task") return "Starting task";
    if (name === "mcp__aichip__list_tasks") return "Checking the board";
    if (name === "mcp__aichip__get_task_status") return "Checking task status";
    if (name === "mcp__aichip__list_agents") return "Browsing agents";
    if (name === "mcp__aichip__cancel_task") return "Stopping the task";
    if (name === "mcp__aichip__get_diff")
      return typeof args.path === "string" ? `Reading the diff: ${args.path}` : "Reading the diff";
    if (name === "mcp__aichip__get_spend") return "Checking what this has cost";
    if (name === "mcp__aichip__list_skills") return "Browsing skills";
    if (name === "mcp__aichip__move_task") return `Filing the card in ${args.column ?? "a column"}`;
    if (name === "Read") return `Reading ${args.file_path ?? "a file"}`;
    if (name === "Grep") return "Searching the codebase";
    if (name === "Glob") return "Listing files";
    return name;
  })();
  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.96 }}
      animate={{ opacity: 1, scale: 1 }}
      className="self-start rounded-full border border-line bg-panel px-3 py-1 text-xs text-ink-dim"
    >
      ⚙ {label}
    </motion.div>
  );
}

function Thinking() {
  return (
    <div className="flex gap-1 self-start rounded-2xl bg-panel-2 px-3 py-2.5">
      {[0, 1, 2].map((i) => (
        <motion.span
          key={i}
          className="h-1.5 w-1.5 rounded-full bg-ink-dim"
          animate={{ opacity: [0.3, 1, 0.3] }}
          transition={{ repeat: Infinity, duration: 1.2, delay: i * 0.2 }}
        />
      ))}
    </div>
  );
}
