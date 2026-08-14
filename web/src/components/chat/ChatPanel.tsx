import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { api, ChatSummary } from "../../lib/api";
import { ChatThread } from "./ChatThread";

/**
 * The docked chat rail: header, conversation switcher, and one `ChatThread`.
 *
 * The thread itself lives in ChatThread so the Chat page can mount the same
 * conversation full-width; this component owns only *which* conversation is
 * open and the list to switch between them.
 */
export function ChatPanel({
  projectId,
  workspaceId,
  projectKind,
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
  /** Passed through so the thread knows whether plan mode means anything
   *  here — a space's chat tools are all read-only. */
  projectKind?: string;
}) {
  const [chatId, setChatId] = useState<string | null>(null);
  const [chats, setChats] = useState<ChatSummary[]>([]);
  const [pickerOpen, setPickerOpen] = useState(false);
  // List-level failures (a delete refused while the assistant is working).
  // The thread has its own banner for send errors; this one is the list's.
  const [listError, setListError] = useState<string | null>(null);

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
    setPickerOpen(false);
    api.openChat(projectId).then((r) => setChatId(r.id)).catch(() => {});
    refreshChats();
  }, [projectId, refreshChats]);

  const switchTo = useCallback((id: string) => {
    setChatId(id);
    setPickerOpen(false);
  }, []);

  const startNewChat = async () => {
    try {
      const r = await api.newChat(projectId);
      switchTo(r.id);
      refreshChats();
    } catch (e) {
      setListError(String(e));
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
      // Usually the 409: the assistant is still working in that chat.
      setListError(String(e));
    }
  };

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
        {listError && (
          <button
            onClick={() => setListError(null)}
            className="mt-1 block w-full rounded-lg bg-red-50 px-3 py-1.5 text-left text-xs text-danger"
            title="Dismiss"
          >
            {listError}
          </button>
        )}

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

      <ChatThread
        projectId={projectId}
        workspaceId={workspaceId}
        projectKind={projectKind}
        chatId={chatId}
        chat={chats.find((c) => c.id === chatId)}
        onSent={refreshChats}
      />
    </div>
  );
}
