import { useEffect, useState } from "react";
import { NavLink } from "react-router-dom";
import { motion } from "framer-motion";
import { nest, TreeNode, TreePage, visibleRows } from "../../lib/kbTree";

/**
 * The page tree.
 *
 * This is the single change that stops a knowledge base feeling like a list of
 * documents: forty equal rectangles become a structure someone decided on. The
 * open/closed set is remembered across visits, because re-expanding your way
 * back to where you were is the fastest way to make a tree feel hostile.
 */
const OPEN_KEY = "aichip.kb.open";

function loadOpen(): Set<string> {
  try {
    return new Set(JSON.parse(localStorage.getItem(OPEN_KEY) ?? "[]"));
  } catch {
    return new Set();
  }
}

export function PageTree({
  pages,
  activeId,
  onCreateChild,
}: {
  pages: TreePage[];
  activeId?: string;
  onCreateChild: (parentId: string) => void;
}) {
  const [open, setOpen] = useState<Set<string>>(loadOpen);

  // Reveal the page you're on, however deep it is — arriving at a page whose
  // row isn't visible is the same as arriving with no tree at all.
  useEffect(() => {
    if (!activeId) return;
    const byId = new Map(pages.map((p) => [p.id, p]));
    const ancestors: string[] = [];
    let cursor = byId.get(activeId);
    for (let i = 0; cursor?.parentId && i < 16; i++) {
      ancestors.push(cursor.parentId);
      cursor = byId.get(cursor.parentId);
    }
    if (!ancestors.length) return;
    setOpen((prev) => {
      if (ancestors.every((a) => prev.has(a))) return prev;
      return new Set([...prev, ...ancestors]);
    });
  }, [activeId, pages]);

  useEffect(() => {
    localStorage.setItem(OPEN_KEY, JSON.stringify([...open]));
  }, [open]);

  const roots = nest(pages);
  const rows = visibleRows(roots, open);

  const toggle = (id: string) =>
    setOpen((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  if (pages.length === 0) {
    return (
      <div className="px-2 py-6 text-center text-xs text-ink-dim">
        No pages in this space yet.
      </div>
    );
  }

  return (
    <div className="flex flex-col">
      {rows.map((node) => (
        <Row
          key={node.id}
          node={node}
          open={open.has(node.id)}
          active={node.id === activeId}
          onToggle={() => toggle(node.id)}
          onCreateChild={() => onCreateChild(node.id)}
        />
      ))}
    </div>
  );
}

function Row({
  node,
  open,
  active,
  onToggle,
  onCreateChild,
}: {
  node: TreeNode;
  open: boolean;
  active: boolean;
  onToggle: () => void;
  onCreateChild: () => void;
}) {
  return (
    <div
      className={`group flex items-center gap-0.5 rounded-lg pr-1 ${
        active ? "bg-accent/10" : "hover:bg-panel-2"
      }`}
      // Indent by depth, clamped: past five levels the extra offset costs more
      // width than it communicates.
      style={{ paddingLeft: 4 + Math.min(node.depth, 5) * 14 }}
    >
      <button
        onClick={onToggle}
        aria-label={open ? "Collapse" : "Expand"}
        className={`w-4 shrink-0 text-[10px] text-ink-dim ${
          node.childCount > 0 ? "" : "invisible"
        }`}
      >
        {open ? "▾" : "▸"}
      </button>
      <NavLink
        to={`/knowledge/${node.id}`}
        className={`min-w-0 flex-1 truncate py-1.5 text-sm ${
          active ? "font-medium text-accent" : ""
        }`}
      >
        <span className="mr-1.5">{node.icon || "▦"}</span>
        {node.title}
      </NavLink>

      {/* Two states worth interrupting for: something is being written, or
          something is waiting on you. Everything else stays quiet. */}
      {node.writing && (
        <motion.span
          className="h-1.5 w-1.5 shrink-0 rounded-full bg-tier-medium"
          animate={{ opacity: [1, 0.3, 1] }}
          transition={{ duration: 1.4, repeat: Infinity }}
          title="an agent is writing this page"
        />
      )}
      {!node.writing && node.hasPending && (
        <span
          className="h-1.5 w-1.5 shrink-0 rounded-full bg-amber-500"
          title="a proposed revision is waiting for you"
        />
      )}
      {node.status === "draft" && (
        <span className="shrink-0 text-[9px] text-ink-dim">draft</span>
      )}

      <button
        onClick={onCreateChild}
        title="Add a page inside this one"
        className="shrink-0 px-1 text-xs text-ink-dim opacity-0 group-hover:opacity-100 hover:text-accent"
      >
        +
      </button>
    </div>
  );
}
