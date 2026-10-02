import { useState } from "react";
import { api } from "../../lib/api";
import { depthOf, legalParents, TreePage } from "../../lib/kbTree";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";

/**
 * Move a page under a different parent.
 *
 * A list rather than drag-to-reparent. Dragging *between* siblings is easy;
 * dragging *into* one means drop-into versus drop-between hit-testing, cycle
 * checks while the pointer moves, and breadcrumbs rebuilding mid-drag — which
 * is where a tree's entire implementation cost lives. This is twenty lines and
 * cannot offer an illegal destination in the first place.
 */
export function MovePicker({
  pages,
  pageId,
  onClose,
  onMoved,
}: {
  pages: TreePage[];
  pageId: string;
  onClose: () => void;
  onMoved: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const legal = legalParents(pages, pageId);

  const move = async (parentId: string | null) => {
    setBusy(true);
    setError(null);
    try {
      await api.movePage(pageId, parentId);
      onMoved();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      width={448}
      title="Move this page"
      description="Its own children move with it. Pages it contains aren't listed — a page cannot live inside itself."
      footer={
        <Button variant="ghost" onClick={onClose}>
          Cancel
        </Button>
      }
    >
      {error && (
        <div className="mb-3 rounded-lg bg-danger-subtle px-3 py-2 text-xs text-danger-fg">
          {error}
        </div>
      )}

      <div className="max-h-72 overflow-y-auto">
        <button
          disabled={busy}
          onClick={() => move(null)}
          className="block w-full rounded-lg px-2.5 py-1.5 text-left text-sm text-fg hover:bg-panel-2 disabled:opacity-50"
        >
          Top level
        </button>
        {legal.map((p) => (
          <button
            key={p.id}
            disabled={busy}
            onClick={() => move(p.id)}
            className="block w-full truncate rounded-lg py-1.5 text-left text-sm text-fg hover:bg-panel-2 disabled:opacity-50"
            style={{ paddingLeft: 10 + depthOf(pages, p.id) * 14 }}
          >
            {p.icon || "▦"} {p.title}
          </button>
        ))}
      </div>
    </Dialog>
  );
}
