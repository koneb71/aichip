import { useCallback, useEffect, useState } from "react";
import { api, ChatSummary } from "../../lib/api";
import { ChatThread } from "./ChatThread";
import { Button, IconButton } from "../ui/Button";
import { Popover } from "../ui/Overlay";

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
    <div className="flex h-full min-h-0 min-w-0 flex-col border-border bg-panel lg:border-r">
      <div className="relative border-b border-border px-4 py-3">
        <div className="flex items-center gap-2">
          <Popover
            open={pickerOpen}
            onOpenChange={setPickerOpen}
            className="max-h-72 w-72 overflow-y-auto p-1!"
            trigger={
              <button
                className="ring-focus flex min-w-0 items-center gap-1.5 rounded-md text-left"
                title="Switch conversation"
              >
                <span className="truncate text-sm font-semibold">
                  {chats.find((c) => c.id === chatId)?.title ?? "Assistant"}
                </span>
                <span className="shrink-0 text-[10px] text-fg-muted">▾</span>
              </button>
            }
          >
            {chats.length === 0 && (
              <div className="px-2 py-2 text-xs text-fg-muted">No conversations yet.</div>
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
                  className="ring-focus min-w-0 flex-1 truncate rounded text-left"
                >
                  {c.title}
                  <span className="ml-1.5 text-[10px] text-fg-muted">{c.messageCount}</span>
                </button>
                <IconButton
                  label="Delete conversation"
                  size="xs"
                  onClick={() => removeChat(c.id)}
                  className="size-5! opacity-0 hover:text-danger-fg focus-visible:opacity-100 group-hover:opacity-100"
                >
                  ✕
                </IconButton>
              </div>
            ))}
          </Popover>
          <Button
            variant="secondary"
            size="xs"
            onClick={startNewChat}
            title="New conversation"
            className="ml-auto"
          >
            + New
          </Button>
        </div>
        <div className="text-[11px] text-fg-muted">
          Asks your own Claude Code to plan &amp; launch tasks
        </div>
        {listError && (
          <button
            onClick={() => setListError(null)}
            className="mt-1 block w-full rounded-lg bg-danger-subtle px-3 py-1.5 text-left text-xs text-danger-fg"
            title="Dismiss"
          >
            {listError}
          </button>
        )}
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
