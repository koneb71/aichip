import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Agent, api, TaskComment } from "../lib/api";
import { useWorkspace } from "../lib/workspace";
import { mentionToken } from "../lib/mention";
import { Markdown } from "./Markdown";

/**
 * The discussion thread under a card. Type `@` to mention an agent — the
 * mentioned agent reads the thread (and the repo) and replies as a comment.
 */
export function TaskComments({ taskId }: { taskId: string }) {
  const { active } = useWorkspace();
  const [comments, setComments] = useState<TaskComment[]>([]);
  const [pending, setPending] = useState(0);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [draft, setDraft] = useState("");
  const [caret, setCaret] = useState(0);
  const [cursor, setCursor] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const boxRef = useRef<HTMLTextAreaElement>(null);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!active) return;
    api.agents(active.id).then((r) => setAgents(r.agents)).catch(() => {});
  }, [active]);

  const refresh = useCallback(async () => {
    try {
      const r = await api.taskComments(taskId);
      setComments(r.comments);
      setPending(r.pendingReplies);
    } catch {
      /* transient */
    }
  }, [taskId]);

  useEffect(() => {
    setComments([]);
    refresh();
    const interval = setInterval(refresh, 3000);
    return () => clearInterval(interval);
  }, [refresh]);

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "nearest" });
  }, [comments.length, pending]);

  // Agent mention: reuse the file-mention tokenizer, filter over agent names.
  const token = useMemo(() => mentionToken(draft, caret), [draft, caret]);
  const candidates = useMemo(() => {
    if (!token) return [];
    const q = token.query.toLowerCase();
    return agents.filter((a) => a.name.toLowerCase().startsWith(q)).slice(0, 6);
  }, [token, agents]);
  const pickerOpen = !!token && candidates.length > 0;

  const insertMention = (agent: Agent) => {
    if (!token) return;
    const next = `${draft.slice(0, token.start)}@${agent.name} ${draft.slice(caret)}`;
    const nextCaret = token.start + agent.name.length + 2;
    setDraft(next);
    setCaret(nextCaret);
    requestAnimationFrame(() => {
      boxRef.current?.setSelectionRange(nextCaret, nextCaret);
      boxRef.current?.focus();
    });
  };

  const send = async () => {
    const content = draft.trim();
    if (!content) return;
    setDraft("");
    setError(null);
    // Optimistic append so the thread answers immediately.
    setComments((prev) => [
      ...prev,
      {
        id: "pending",
        author: "user",
        agentId: null,
        agentName: null,
        agentColor: null,
        content,
        runId: null,
        filePath: null,
        line: null,
        hunk: null,
        ts: new Date().toISOString(),
      },
    ]);
    try {
      const r = await api.postComment(taskId, content);
      if (r.runIds.length > 0) setPending((p) => p + r.runIds.length);
      refresh();
    } catch (e) {
      setError(String(e));
      refresh();
    }
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="min-h-0 flex-1 overflow-y-auto">
        {comments.length === 0 && pending === 0 && (
          <div className="px-1 py-6 text-center text-sm text-ink-dim">
            No comments yet. Mention an agent with @ to ask it something — it
            reads the repo before answering.
          </div>
        )}
        <div className="flex flex-col gap-3">
          {comments.map((c) => (
            <CommentRow key={c.id + c.ts} comment={c} />
          ))}
          {pending > 0 && (
            <div className="flex items-center gap-2 text-xs text-ink-dim">
              <motion.span
                className="h-2 w-2 rounded-full bg-accent"
                animate={{ opacity: [0.3, 1, 0.3] }}
                transition={{ repeat: Infinity, duration: 1.2 }}
              />
              {pending === 1 ? "an agent is replying…" : `${pending} agents are replying…`}
            </div>
          )}
          <div ref={endRef} />
        </div>
      </div>

      {error && (
        <div className="mb-1 rounded-lg bg-red-50 px-3 py-1.5 text-xs text-danger">{error}</div>
      )}

      <div className="relative mt-2">
        <AnimatePresence>
          {pickerOpen && (
            <motion.div
              initial={{ opacity: 0, y: 4 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 4 }}
              className="absolute bottom-full left-0 right-0 z-20 mb-1 rounded-xl border border-line bg-panel p-1 shadow-lg"
            >
              {candidates.map((a, i) => (
                <button
                  key={a.id}
                  onMouseEnter={() => setCursor(i)}
                  onMouseDown={(e) => {
                    e.preventDefault();
                    insertMention(a);
                  }}
                  className={`flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-sm ${
                    i === cursor ? "bg-panel-2" : ""
                  }`}
                >
                  <span
                    className="h-2.5 w-2.5 shrink-0 rounded-full"
                    style={{ background: a.color }}
                  />
                  <span className="min-w-0 flex-1 truncate">{a.name}</span>
                  <span className="shrink-0 truncate text-[11px] text-ink-dim">
                    {a.description}
                  </span>
                </button>
              ))}
            </motion.div>
          )}
        </AnimatePresence>

        <div className="flex items-end gap-2 rounded-xl border border-line bg-panel px-3 py-2 focus-within:border-accent">
          <textarea
            ref={boxRef}
            value={draft}
            onChange={(e) => {
              setDraft(e.target.value);
              setCaret(e.target.selectionStart ?? 0);
              setCursor(0);
            }}
            onSelect={(e) => setCaret(e.currentTarget.selectionStart ?? 0)}
            onKeyDown={(e) => {
              if (pickerOpen) {
                if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                  e.preventDefault();
                  const d = e.key === "ArrowDown" ? 1 : -1;
                  setCursor((c) => (c + d + candidates.length) % candidates.length);
                  return;
                }
                if (e.key === "Enter" || e.key === "Tab") {
                  e.preventDefault();
                  insertMention(candidates[cursor] ?? candidates[0]);
                  return;
                }
              }
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                send();
              }
            }}
            rows={Math.min(4, Math.max(1, draft.split("\n").length))}
            placeholder="Comment — @ mentions an agent"
            className="min-w-0 flex-1 resize-none bg-transparent text-sm outline-none"
          />
          <motion.button
            whileTap={{ scale: 0.92 }}
            onClick={send}
            disabled={!draft.trim()}
            className="rounded-lg bg-accent px-2.5 py-1.5 text-sm text-white disabled:opacity-40"
          >
            ↑
          </motion.button>
        </div>
      </div>
    </div>
  );
}

function CommentRow({ comment }: { comment: TaskComment }) {
  if (comment.author === "user") {
    return (
      <motion.div
        initial={{ opacity: 0, y: 4 }}
        animate={{ opacity: 1, y: 0 }}
        className="max-w-[90%] self-end"
      >
        {/* A note written against the diff says so, or it reads as a general
            remark once you have scrolled away from the code. */}
        {comment.filePath && (
          <div className="mb-1 truncate text-right font-mono text-[11px] text-ink-dim">
            {comment.filePath}
            {comment.line ? `:${comment.line}` : ""}
          </div>
        )}
        <div className="rounded-2xl rounded-br-sm bg-accent px-3 py-2 text-sm whitespace-pre-wrap text-white">
          {comment.content}
        </div>
      </motion.div>
    );
  }
  return (
    <motion.div
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      className="max-w-[92%] self-start"
    >
      <div className="mb-1 flex items-center gap-1.5 text-[11px] text-ink-dim">
        <span
          className="h-2 w-2 rounded-full"
          style={{ background: comment.agentColor ?? "#9ca3af" }}
        />
        {comment.agentName ?? "agent"}
      </div>
      <div className="rounded-2xl rounded-bl-sm bg-panel-2 px-3 py-2 text-sm">
        <Markdown>{comment.content}</Markdown>
      </div>
    </motion.div>
  );
}
