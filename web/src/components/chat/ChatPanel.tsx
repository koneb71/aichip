import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { api, Agent, ChatMessage, ChatSummary, Effort, Tier } from "../../lib/api";
import { useRunStream } from "../../lib/ws";
import { useAttachments } from "../../lib/useAttachments";
import { agentSpans } from "../../lib/mention";
import { AttachmentBar, AttachmentList } from "../AttachmentBar";
import { ComposerSettings } from "./ComposerSettings";
import { useMentionPicker } from "../MentionPicker";
import { Markdown } from "../Markdown";

export function ChatPanel({
  projectId,
  workspaceId,
}: {
  projectId: string;
  /**
   * The workspace this *project* belongs to — not whichever one the sidebar is
   * showing. Those differ: switching workspace does not navigate away from an
   * open project, and a bookmarked link opens one while the switcher sits on
   * the first workspace in the list. The server resolves `@mentions` against
   * the project's workspace, so resolving them here against any other list
   * would offer agents that cannot bind and draw chips for mentions that
   * did not.
   */
  workspaceId?: string;
}) {
  const [chatId, setChatId] = useState<string | null>(null);
  const [chats, setChats] = useState<ChatSummary[]>([]);
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
  const [pickerOpen, setPickerOpen] = useState(false);
  // The agent library, fetched once. Both the `@` picker and the message
  // bubbles resolve names against this one list, so a chip is only ever drawn
  // for an agent that exists.
  const [agents, setAgents] = useState<Agent[]>([]);
  const scrollRef = useRef<HTMLDivElement>(null);
  const streamEvents = useRunStream(activeRunId);
  const att = useAttachments(projectId);
  const composerRef = useRef<HTMLTextAreaElement>(null);
  // Caret is tracked separately: it moves on click and arrow keys, not just
  // on change, and the mention token depends on where it is.
  const [caret, setCaret] = useState(0);
  const mention = useMentionPicker({
    projectId,
    agents,
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
    api.agents(workspaceId).then((r) => setAgents(r.agents)).catch(() => {});
  }, [workspaceId]);

  const agentNames = useMemo(() => agents.map((a) => a.name), [agents]);

  const refreshChats = useCallback(
    () =>
      api
        .chats(projectId)
        .then((r) => setChats(r.chats))
        .catch(() => {}),
    [projectId],
  );

  useEffect(() => {
    setChatId(null);
    setChats([]);
    setMessages([]);
    setActiveRunId(null);
    setPickerOpen(false);
    api.openChat(projectId).then((r) => setChatId(r.id)).catch(() => {});
    refreshChats();
  }, [projectId, refreshChats]);

  // A conversation remembers what it was last run with, so reopening it picks
  // up where you left off rather than snapping back to the defaults. Guarded by
  // the ref so a routine refresh of the chat list can't undo a choice you made
  // in the composer but haven't sent yet.
  const seededFor = useRef<string | null>(null);
  useEffect(() => {
    if (!chatId || seededFor.current === chatId) return;
    const chat = chats.find((c) => c.id === chatId);
    if (!chat) return;
    seededFor.current = chatId;
    setTier(chat.modelTier ?? "medium");
    setEffort(chat.effort);
  }, [chatId, chats]);

  // Switching conversations must drop the previous thread's messages and
  // stream, or the old run's text bleeds into the new chat.
  const switchTo = useCallback((id: string) => {
    setChatId(id);
    setMessages([]);
    setActiveRunId(null);
    setError(null);
    setPickerOpen(false);
  }, []);

  const startNewChat = async () => {
    try {
      const r = await api.newChat(projectId);
      switchTo(r.id);
      refreshChats();
    } catch (e) {
      setError(String(e));
    }
  };

  const removeChat = async (id: string) => {
    try {
      await api.deleteChat(id);
      const remaining = chats.filter((c) => c.id !== id);
      setChats(remaining);
      if (id === chatId) {
        // Fall back to whatever is left, creating one if the list is empty.
        if (remaining[0]) switchTo(remaining[0].id);
        else await api.openChat(projectId).then((r) => switchTo(r.id));
      }
      refreshChats();
    } catch (e) {
      setError(String(e));
    }
  };

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
      // The first message names the chat server-side — pick that title up.
      refreshChats();
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

  return (
    // `lg:border-r` only: the divider separates the docked column from the
    // board, and reads as a stray line when the panel is a narrow-screen tab.
    <div className="flex h-full min-h-0 min-w-0 flex-col border-line bg-panel lg:border-r">
      <div className="relative border-b border-line px-4 py-3">
        <div className="flex items-center gap-2">
          <button
            onClick={() => setPickerOpen((o) => !o)}
            className="flex min-w-0 items-center gap-1.5 text-left"
            title="Switch conversation"
          >
            <span className="truncate text-sm font-semibold">
              {chats.find((c) => c.id === chatId)?.title ?? "Assistant"}
            </span>
            <span className="shrink-0 text-[10px] text-ink-dim">▾</span>
          </button>
          <button
            onClick={startNewChat}
            title="New conversation"
            className="ml-auto shrink-0 rounded-lg border border-line px-2 py-0.5 text-xs text-ink-dim hover:bg-panel-2 hover:text-ink"
          >
            + New
          </button>
        </div>
        <div className="text-[11px] text-ink-dim">
          Asks your own Claude Code to plan &amp; launch tasks
        </div>

        <AnimatePresence>
          {pickerOpen && (
            <>
              {/* Click-away layer, below the menu but above the panel. */}
              <div className="fixed inset-0 z-10" onClick={() => setPickerOpen(false)} />
              <motion.div
                initial={{ opacity: 0, y: -4 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -4 }}
                className="absolute left-3 right-3 top-full z-20 max-h-72 overflow-y-auto rounded-xl border border-line bg-panel p-1 shadow-lg"
              >
                {chats.length === 0 && (
                  <div className="px-2 py-2 text-xs text-ink-dim">No conversations yet.</div>
                )}
                {chats.map((c) => (
                  <div
                    key={c.id}
                    className={`group flex items-center gap-1 rounded-lg px-2 py-1.5 text-sm ${
                      c.id === chatId ? "bg-panel-2 font-medium" : "hover:bg-panel-2"
                    }`}
                  >
                    <button
                      onClick={() => switchTo(c.id)}
                      className="min-w-0 flex-1 truncate text-left"
                    >
                      {c.title}
                      <span className="ml-1.5 text-[10px] text-ink-dim">{c.messageCount}</span>
                    </button>
                    <button
                      onClick={() => removeChat(c.id)}
                      title="Delete conversation"
                      className="shrink-0 px-1 text-xs text-ink-dim opacity-0 hover:text-danger group-hover:opacity-100"
                    >
                      ✕
                    </button>
                  </div>
                ))}
              </motion.div>
            </>
          )}
        </AnimatePresence>
      </div>

      <div ref={scrollRef} className="min-h-0 flex-1 overflow-y-auto p-4">
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
      </div>

      {error && (
        <div className="mx-4 mb-1 rounded-lg bg-red-50 px-3 py-1.5 text-xs text-danger">
          {error}
        </div>
      )}

      <div className="relative border-t border-line p-3" {...att.dropProps}>
        {mention.node}
        <div
          className={`flex flex-col gap-1.5 rounded-xl border bg-panel px-3 py-2 focus-within:border-accent ${
            att.dragging ? "border-accent ring-2 ring-accent/30" : "border-line"
          }`}
        >
          {(att.items.length > 0 || att.dragging) && (
            <AttachmentBar
              items={att.items}
              onAdd={att.add}
              onRemove={att.remove}
              full={att.full}
              disabled={!!activeRunId}
            />
          )}
          <div className="flex items-end gap-2">
            {att.items.length === 0 && !att.dragging && (
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
                if (mention.handleKey(e)) {
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
      </div>
    </div>
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
