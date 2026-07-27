import { useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { api } from "../../lib/api";
import { useWorkspace } from "../../lib/workspace";

export function WorkspaceSwitcher() {
  const { workspaces, active, setActive, refresh } = useWorkspace();
  const [open, setOpen] = useState(false);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");

  const create = async () => {
    if (!name.trim()) return;
    const { id } = await api.createWorkspace(name.trim());
    await refresh();
    setActive(id);
    setName("");
    setCreating(false);
    setOpen(false);
  };

  return (
    <div className="relative">
      <button
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center gap-2 rounded-lg px-2 py-1.5 hover:bg-panel-2"
      >
        <span
          className="flex h-7 w-7 items-center justify-center rounded-lg text-xs font-semibold text-white"
          style={{ background: active?.color ?? "var(--color-accent)" }}
        >
          {(active?.name ?? "W")
            .split(" ")
            .map((w) => w[0])
            .slice(0, 2)
            .join("")}
        </span>
        <span className="min-w-0 flex-1 truncate text-left text-sm font-semibold">
          {active?.name ?? "Workspace"}
        </span>
        <svg width="12" height="12" viewBox="0 0 12 12" className="text-ink-dim">
          <path d="M3 4.5 6 7.5 9 4.5" fill="none" stroke="currentColor" strokeWidth="1.5" />
        </svg>
      </button>

      <AnimatePresence>
        {open && (
          <motion.div
            initial={{ opacity: 0, y: -4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -4 }}
            className="card-shadow absolute left-0 right-0 top-full z-30 mt-1 rounded-xl border border-line bg-panel p-1"
          >
            {workspaces.map((w) => (
              <button
                key={w.id}
                onClick={() => {
                  setActive(w.id);
                  setOpen(false);
                }}
                className={`flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-sm hover:bg-panel-2 ${
                  w.id === active?.id ? "font-semibold" : ""
                }`}
              >
                <span
                  className="h-2.5 w-2.5 rounded-full"
                  style={{ background: w.color }}
                />
                <span className="truncate">{w.name}</span>
                {w.id === active?.id && <span className="ml-auto text-xs">✓</span>}
              </button>
            ))}
            <div className="my-1 border-t border-line" />
            {creating ? (
              <div className="flex items-center gap-1 p-1">
                <input
                  autoFocus
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  onKeyDown={(e) => e.key === "Enter" && create()}
                  placeholder="Workspace name"
                  className="min-w-0 flex-1 rounded-lg border border-line bg-panel px-2 py-1 text-sm outline-none focus:border-accent"
                />
                <button onClick={create} className="rounded-lg bg-accent px-2 py-1 text-xs text-white">
                  Add
                </button>
              </div>
            ) : (
              <button
                onClick={() => setCreating(true)}
                className="w-full rounded-lg px-2 py-1.5 text-left text-sm text-ink-dim hover:bg-panel-2"
              >
                + New workspace
              </button>
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
