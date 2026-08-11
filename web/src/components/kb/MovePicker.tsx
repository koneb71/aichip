import { useState } from "react";
import { motion } from "framer-motion";
import { api } from "../../lib/api";
import { depthOf, legalParents, TreePage } from "../../lib/kbTree";

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
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/25 backdrop-blur-[3px] p-4"
      onClick={onClose}
    >
      <motion.div
        initial={{ y: 16, scale: 0.97, opacity: 0 }}
        animate={{ y: 0, scale: 1, opacity: 1 }}
        transition={{ type: "spring", stiffness: 220, damping: 26 }}
        onClick={(e) => e.stopPropagation()}
        className="card-shadow w-full max-w-md rounded-2xl border border-line bg-panel p-5"
      >
        <div className="text-base font-semibold">Move this page</div>
        <p className="mt-1 text-xs text-ink-dim">
          Its own children move with it. Pages it contains aren't listed — a page
          cannot live inside itself.
        </p>

        {error && (
          <div className="mt-3 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">
            {error}
          </div>
        )}

        <div className="mt-3 max-h-72 overflow-y-auto">
          <button
            disabled={busy}
            onClick={() => move(null)}
            className="block w-full rounded-lg px-2.5 py-1.5 text-left text-sm hover:bg-panel-2 disabled:opacity-50"
          >
            Top level
          </button>
          {legal.map((p) => (
            <button
              key={p.id}
              disabled={busy}
              onClick={() => move(p.id)}
              className="block w-full truncate rounded-lg py-1.5 text-left text-sm hover:bg-panel-2 disabled:opacity-50"
              style={{ paddingLeft: 10 + depthOf(pages, p.id) * 14 }}
            >
              {p.icon || "▦"} {p.title}
            </button>
          ))}
        </div>

        <div className="mt-4 flex justify-end">
          <button
            onClick={onClose}
            className="rounded-lg px-4 py-2 text-sm text-ink-dim hover:text-ink"
          >
            Cancel
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}
