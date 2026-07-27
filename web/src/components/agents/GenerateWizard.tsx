import { useState } from "react";
import { motion } from "framer-motion";
import { AgentDraft, api, Tier, tierColor, tierModel, tierSoft } from "../../lib/api";

type Phase = "describe" | "generating" | "review";

export function GenerateWizard({
  workspaceId,
  onClose,
  onSaved,
}: {
  workspaceId: string;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [phase, setPhase] = useState<Phase>("describe");
  const [description, setDescription] = useState("");
  const [drafts, setDrafts] = useState<AgentDraft[]>([]);
  const [saved, setSaved] = useState<Set<number>>(new Set());
  const [error, setError] = useState<string | null>(null);

  const generate = async () => {
    if (!description.trim()) return;
    setPhase("generating");
    setError(null);
    try {
      const r = await api.generateAgents(description.trim());
      setDrafts(r.drafts);
      setSaved(new Set());
      setPhase("review");
    } catch (e) {
      setError(String(e));
      setPhase("describe");
    }
  };

  const saveDraft = async (index: number) => {
    const d = drafts[index];
    try {
      await api.createAgent({
        workspace_id: workspaceId,
        name: d.name,
        icon: d.icon ?? "bot",
        color: d.color ?? "#4f46e5",
        description: d.description ?? "",
        system_prompt: d.system_prompt ?? "",
        model_tier: d.model_tier ?? "medium",
        permission_preset: d.permission_preset ?? "reviewed",
        allowed_tools: d.allowed_tools ?? [],
      });
      setSaved((prev) => new Set(prev).add(index));
      onSaved();
    } catch (e) {
      setError(String(e));
    }
  };

  const editDraft = (index: number, patch: Partial<AgentDraft>) =>
    setDrafts((prev) => prev.map((d, i) => (i === index ? { ...d, ...patch } : d)));

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 p-6"
      onClick={onClose}
    >
      <motion.div
        initial={{ y: 20, scale: 0.98 }}
        animate={{ y: 0, scale: 1 }}
        exit={{ y: 20, scale: 0.98 }}
        transition={{ type: "spring", stiffness: 380, damping: 30 }}
        onClick={(e) => e.stopPropagation()}
        className="card-shadow flex max-h-[80vh] w-full max-w-2xl flex-col rounded-2xl border border-line bg-panel"
      >
        <div className="border-b border-line p-5">
          <div className="text-base font-semibold">✦ Generate agents with AI</div>
          <div className="mt-0.5 text-xs text-ink-dim">
            Runs on your own Claude Code (Fable tier). Drafts are yours to edit — nothing
            is saved until you say so.
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-5">
          {phase === "describe" && (
            <>
              <textarea
                autoFocus
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                rows={4}
                placeholder="Describe what you need… e.g. “a team that triages GitHub issues, fixes the easy ones, and drafts PRs with tests”"
                className="w-full resize-none rounded-xl border border-line bg-panel px-3 py-2.5 text-sm outline-none focus:border-accent"
              />
              {error && (
                <div className="mt-3 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">
                  {error}
                </div>
              )}
            </>
          )}

          {phase === "generating" && (
            <div className="flex flex-col items-center gap-3 py-12">
              <motion.div
                className="h-8 w-8 rounded-full border-2 border-accent border-t-transparent"
                animate={{ rotate: 360 }}
                transition={{ repeat: Infinity, duration: 0.9, ease: "linear" }}
              />
              <div className="text-sm text-ink-dim">
                Designing your agents… this uses one Fable request.
              </div>
            </div>
          )}

          {phase === "review" && (
            <div className="flex flex-col gap-4">
              {drafts.map((d, i) => (
                <div key={i} className="rounded-xl border border-line p-4">
                  <div className="flex items-center gap-2">
                    <input
                      value={d.name}
                      onChange={(e) => editDraft(i, { name: e.target.value })}
                      className="min-w-0 flex-1 rounded-lg border border-line px-2 py-1 text-sm font-semibold outline-none focus:border-accent"
                    />
                    <select
                      value={d.model_tier ?? "medium"}
                      onChange={(e) => editDraft(i, { model_tier: e.target.value as Tier })}
                      className="rounded-lg border border-line px-2 py-1 text-xs"
                      style={{
                        background: tierSoft[(d.model_tier ?? "medium") as Tier],
                        color: tierColor[(d.model_tier ?? "medium") as Tier],
                      }}
                    >
                      {(["easy", "medium", "complex"] as Tier[]).map((t) => (
                        <option key={t} value={t}>
                          {tierModel[t]}
                        </option>
                      ))}
                    </select>
                  </div>
                  <input
                    value={d.description ?? ""}
                    onChange={(e) => editDraft(i, { description: e.target.value })}
                    className="mt-2 w-full rounded-lg border border-line px-2 py-1 text-xs text-ink-dim outline-none focus:border-accent"
                  />
                  <textarea
                    value={d.system_prompt ?? ""}
                    onChange={(e) => editDraft(i, { system_prompt: e.target.value })}
                    rows={3}
                    className="mt-2 w-full resize-none rounded-lg border border-line px-2 py-1.5 text-xs outline-none focus:border-accent"
                  />
                  <div className="mt-2 flex justify-end">
                    {saved.has(i) ? (
                      <span className="text-sm text-tier-easy">✓ Saved</span>
                    ) : (
                      <motion.button
                        whileTap={{ scale: 0.96 }}
                        onClick={() => saveDraft(i)}
                        className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white"
                      >
                        Save agent
                      </motion.button>
                    )}
                  </div>
                </div>
              ))}
              {error && (
                <div className="rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">{error}</div>
              )}
            </div>
          )}
        </div>

        <div className="flex justify-end gap-2 border-t border-line p-4">
          <button onClick={onClose} className="rounded-lg px-4 py-2 text-sm text-ink-dim hover:text-ink">
            {phase === "review" ? "Done" : "Cancel"}
          </button>
          {phase === "describe" && (
            <motion.button
              whileTap={{ scale: 0.96 }}
              onClick={generate}
              disabled={!description.trim()}
              className="rounded-lg bg-accent px-5 py-2 text-sm font-medium text-white disabled:opacity-50"
            >
              Generate
            </motion.button>
          )}
          {phase === "review" && (
            <motion.button
              whileTap={{ scale: 0.96 }}
              onClick={() => setPhase("describe")}
              className="rounded-lg border border-line px-4 py-2 text-sm"
            >
              ↻ Regenerate
            </motion.button>
          )}
        </div>
      </motion.div>
    </motion.div>
  );
}
