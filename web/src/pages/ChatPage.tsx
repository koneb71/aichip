import { useCallback, useEffect, useState } from "react";
import { Link, useSearchParams } from "react-router-dom";
import { api, ChatSummary, Project } from "../lib/api";
import { useWorkspace } from "../lib/workspace";
import { NARROW, useMediaQuery } from "../lib/useMediaQuery";
import { ChatThread } from "../components/chat/ChatThread";

/**
 * Chat as a page: the conversation list on the left, one thread full-width.
 *
 * The same machinery as the project page's rail — `ChatThread` is shared —
 * with the parts a 380px column has no room for: an always-visible list,
 * rename, and a reading-width thread. Chats stay project-scoped (the server
 * resolves everything through the chat's project), so the rail opens with a
 * project picker.
 */
const PROJECT_KEY = "aichip.chat.project";

export default function ChatPage() {
  const { active } = useWorkspace();
  const narrow = useMediaQuery(NARROW);
  const [params, setParams] = useSearchParams();

  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState<string | null>(null);
  const [chats, setChats] = useState<ChatSummary[]>([]);
  const [chatId, setChatId] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [renameDraft, setRenameDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [railOpen, setRailOpen] = useState(false);

  // Which project: the URL wins (a shared link means *this* project), then
  // the last choice, then the most recent project. The URL is kept in sync so
  // the current view is always linkable.
  useEffect(() => {
    if (!active) return;
    api
      .projects(active.id)
      .then((r) => {
        setProjects(r.projects);
        const fromUrl = params.get("project");
        const remembered = localStorage.getItem(PROJECT_KEY);
        const pick =
          r.projects.find((p) => p.id === fromUrl)?.id ??
          r.projects.find((p) => p.id === remembered)?.id ??
          r.projects[0]?.id ??
          null;
        setProjectId(pick);
      })
      .catch(() => {});
    // params deliberately not a dependency: the URL is an input once, then an
    // output — reacting to our own setParams would loop.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active]);

  const pickProject = (id: string) => {
    setProjectId(id);
    setChatId(null);
    setChats([]);
    localStorage.setItem(PROJECT_KEY, id);
    setParams({ project: id }, { replace: true });
  };

  const refreshChats = useCallback(() => {
    if (!projectId) return Promise.resolve();
    return api
      .chats(projectId)
      .then((r) => setChats(r.chats))
      .catch(() => {});
  }, [projectId]);

  useEffect(() => {
    if (!projectId) return;
    setChatId(null);
    api.openChat(projectId).then((r) => setChatId(r.id)).catch(() => {});
    refreshChats();
  }, [projectId, refreshChats]);

  const startNewChat = async () => {
    if (!projectId) return;
    try {
      const r = await api.newChat(projectId);
      setChatId(r.id);
      refreshChats();
    } catch (e) {
      setError(String(e));
    }
  };

  const removeChat = async (id: string) => {
    if (!projectId) return;
    try {
      await api.deleteChat(id);
      const remaining = chats.filter((c) => c.id !== id);
      setChats(remaining);
      if (id === chatId) {
        if (remaining[0]) setChatId(remaining[0].id);
        else await api.openChat(projectId).then((r) => setChatId(r.id));
      }
      refreshChats();
    } catch (e) {
      // Usually the 409: the assistant is still working in that chat.
      setError(String(e));
    }
  };

  const commitRename = async (id: string) => {
    const title = renameDraft.trim();
    setRenaming(null);
    // An emptied field is a cancel, not a request — the server 400s empty.
    if (!title || title === chats.find((c) => c.id === id)?.title) return;
    try {
      await api.renameChat(id, title);
      refreshChats();
    } catch (e) {
      setError(String(e));
    }
  };

  const project = projects.find((p) => p.id === projectId);

  if (active && projects.length === 0) {
    return (
      <div className="flex h-full items-center justify-center p-8 text-center">
        <div>
          <div className="text-sm font-medium">No projects yet</div>
          <div className="mt-1 text-sm text-ink-dim">
            Chat works inside a project —{" "}
            <Link to="/projects" className="text-accent underline">
              add one
            </Link>{" "}
            first.
          </div>
        </div>
      </div>
    );
  }

  const rail = (
    <div className="flex min-h-0 flex-col gap-3 p-3">
      <select
        value={projectId ?? ""}
        onChange={(e) => pickProject(e.target.value)}
        className="w-full rounded-lg border border-line bg-panel px-2 py-1.5 text-sm"
        title="Chats are scoped to a project"
      >
        {projects.map((p) => (
          <option key={p.id} value={p.id}>
            {p.name}
          </option>
        ))}
      </select>
      <button
        onClick={startNewChat}
        className="rounded-lg border border-line px-2 py-1.5 text-sm text-ink-dim hover:bg-panel-2 hover:text-ink"
      >
        + New conversation
      </button>
      <div className="min-h-0 flex-1 overflow-y-auto">
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
            {renaming === c.id ? (
              <input
                autoFocus
                value={renameDraft}
                onChange={(e) => setRenameDraft(e.target.value)}
                onBlur={() => commitRename(c.id)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") commitRename(c.id);
                  if (e.key === "Escape") setRenaming(null);
                }}
                className="min-w-0 flex-1 rounded border border-accent bg-panel px-1 py-0.5 text-sm outline-none"
              />
            ) : (
              <button
                onClick={() => {
                  setChatId(c.id);
                  setRailOpen(false);
                }}
                onDoubleClick={() => {
                  setRenaming(c.id);
                  setRenameDraft(c.title);
                }}
                className="min-w-0 flex-1 truncate text-left"
                title="Double-click to rename"
              >
                {c.title}
                <span className="ml-1.5 text-[10px] text-ink-dim">{c.messageCount}</span>
              </button>
            )}
            <button
              onClick={() => {
                setRenaming(c.id);
                setRenameDraft(c.title);
              }}
              title="Rename"
              className="shrink-0 px-1 text-xs text-ink-dim opacity-0 hover:text-ink group-hover:opacity-100"
            >
              ✎
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
      </div>
    </div>
  );

  const thread = projectId && (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
      {error && (
        <button
          onClick={() => setError(null)}
          className="mx-4 mt-2 rounded-lg bg-red-50 px-3 py-1.5 text-left text-xs text-danger"
          title="Dismiss"
        >
          {error}
        </button>
      )}
      <ChatThread
        projectId={projectId}
        workspaceId={project?.workspaceId ?? active?.id}
        chatId={chatId}
        chat={chats.find((c) => c.id === chatId)}
        onSent={refreshChats}
        centered
      />
    </div>
  );

  if (narrow) {
    return (
      <div className="flex h-full min-h-0 flex-col">
        <button
          onClick={() => setRailOpen((o) => !o)}
          className="border-b border-line px-4 py-2 text-left text-sm font-medium"
        >
          {chats.find((c) => c.id === chatId)?.title ?? "Conversations"}{" "}
          <span className="text-[10px] text-ink-dim">{railOpen ? "▴" : "▾"}</span>
        </button>
        {railOpen && <div className="max-h-64 overflow-y-auto border-b border-line">{rail}</div>}
        {thread}
      </div>
    );
  }

  return (
    <div className="grid h-full min-h-0 grid-cols-[280px_minmax(0,1fr)]">
      <div className="min-h-0 overflow-hidden border-r border-line bg-panel">{rail}</div>
      {thread}
    </div>
  );
}
