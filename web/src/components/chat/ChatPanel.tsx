import { useCallback, useEffect, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { api, ChatMessage, ChatSummary } from "../../lib/api";
import { useRunStream } from "../../lib/ws";
import { Markdown } from "../Markdown";

export function ChatPanel({ projectId }: { projectId: string }) {
  const [chatId, setChatId] = useState<string | null>(null);
  const [chats, setChats] = useState<ChatSummary[]>([]);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [activeRunId, setActiveRunId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);
  const streamEvents = useRunStream(activeRunId);

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
    if (!chatId || !draft.trim() || activeRunId) return;
    setError(null);
    const content = draft.trim();
    setDraft("");
    // Optimistic append.
    setMessages((prev) => [
      ...prev,
      { id: "pending", role: "user", content, runId: null, ts: new Date().toISOString() },
    ]);
    try {
      const r = await api.sendChat(chatId, content);
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
    <div className="flex h-full min-h-0 min-w-0 flex-col border-r border-line bg-panel">
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
            keeps you posted here.
          </div>
        )}
        <div className="flex flex-col gap-3">
          {messages.map((m) => (
            <Message key={m.id + m.ts} message={m} />
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

      <div className="border-t border-line p-3">
        <div className="flex items-end gap-2 rounded-xl border border-line bg-panel px-3 py-2 focus-within:border-accent">
          <textarea
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                send();
              }
            }}
            rows={Math.min(4, Math.max(1, draft.split("\n").length))}
            placeholder={activeRunId ? "Assistant is working…" : "What should we work on?"}
            disabled={!!activeRunId}
            className="min-w-0 flex-1 resize-none bg-transparent text-sm outline-none disabled:opacity-60"
          />
          <motion.button
            whileTap={{ scale: 0.92 }}
            onClick={send}
            disabled={!!activeRunId || !draft.trim()}
            className="rounded-lg bg-accent px-2.5 py-1.5 text-sm text-white disabled:opacity-40"
          >
            ↑
          </motion.button>
        </div>
      </div>
    </div>
  );
}

function Message({ message }: { message: ChatMessage }) {
  if (message.role === "user") {
    return (
      <motion.div
        initial={{ opacity: 0, y: 4 }}
        animate={{ opacity: 1, y: 0 }}
        className="max-w-[85%] self-end rounded-2xl rounded-br-sm bg-accent px-3 py-2 text-sm text-white whitespace-pre-wrap"
      >
        {message.content}
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

function ToolChip({ name, input }: { name: string; input: unknown }) {
  const label = (() => {
    const args = (input ?? {}) as Record<string, unknown>;
    if (name === "mcp__aichip__create_task") return `Creating task: ${args.title ?? ""}`;
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
