import { useState } from "react";
import { motion } from "framer-motion";
import { AgentDraft, api, Tier, tierColor, tierSoft } from "../../lib/api";
import { useTierModel } from "../../lib/models";
import { TIERS, TierPicker } from "../TierPicker";
import { DEFAULT_AGENT_COLOR } from "../../lib/swatches";
import { Dialog } from "../ui/Dialog";
import { Button } from "../ui/Button";

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
  const tierModel = useTierModel();
  const [phase, setPhase] = useState<Phase>("describe");
  const [description, setDescription] = useState("");
  // Which model does the designing. Complex by default, so nothing about the
  // existing behaviour changes for somebody who never opens this — but it is
  // no longer the only option, and the label names what it will actually cost.
  const [tier, setTier] = useState<Tier>("complex");
  const [drafts, setDrafts] = useState<AgentDraft[]>([]);
  const [saved, setSaved] = useState<Set<number>>(new Set());
  const [error, setError] = useState<string | null>(null);

  const generate = async () => {
    if (!description.trim()) return;
    setPhase("generating");
    setError(null);
    try {
      const r = await api.generateAgents(description.trim(), undefined, tier);
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
        color: d.color ?? DEFAULT_AGENT_COLOR,
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
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      width={672}
      title="✦ Generate agents with AI"
      description={
        <>
          Runs on your own Claude Code login. Drafts are yours to edit — nothing is
          saved until you say so.
        </>
      }
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            {phase === "review" ? "Done" : "Cancel"}
          </Button>
          {phase === "describe" && (
            <Button variant="primary" onClick={generate} disabled={!description.trim()}>
              Generate
            </Button>
          )}
          {phase === "review" && (
            <Button onClick={() => setPhase("describe")}>↻ Regenerate</Button>
          )}
        </>
      }
    >
      {phase === "describe" && (
        <>
          <textarea
            autoFocus
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            rows={4}
            placeholder="Describe what you need… e.g. “a team that triages GitHub issues, fixes the easy ones, and drafts PRs with tests”"
            className="w-full resize-none rounded-xl border border-border bg-panel px-3 py-2.5 text-sm outline-none focus:border-accent"
          />
          {/* Designing a team is one-shot judgement, which thinking time
              serves better than model size — so this is worth choosing
              rather than always paying for the largest one. The picker
              names the model each tier resolves to, since "Complex" alone
              does not tell you what you are about to spend. */}
          <label className="mt-3 flex items-center gap-2 text-xs text-fg-muted">
            Designed by
            <TierPicker value={tier} onChange={setTier} />
          </label>
          {error && (
            <div className="mt-3 rounded-lg bg-danger-subtle px-3 py-2 text-xs text-danger-fg">
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
          <div className="text-sm text-fg-muted">
            Designing your agents… this uses one Fable request.
          </div>
        </div>
      )}

      {phase === "review" && (
        <div className="flex flex-col gap-4">
          {drafts.map((d, i) => (
            <div key={i} className="rounded-xl border border-border p-4">
              <div className="flex items-center gap-2">
                <input
                  value={d.name}
                  onChange={(e) => editDraft(i, { name: e.target.value })}
                  className="min-w-0 flex-1 rounded-lg border border-border bg-panel px-2 py-1 text-sm font-semibold outline-none focus:border-accent"
                />
                <select
                  value={d.model_tier ?? "medium"}
                  onChange={(e) => editDraft(i, { model_tier: e.target.value as Tier })}
                  className="rounded-lg border border-border px-2 py-1 text-xs"
                  style={{
                    background: tierSoft[(d.model_tier ?? "medium") as Tier],
                    color: tierColor[(d.model_tier ?? "medium") as Tier],
                  }}
                >
                  {TIERS.map((t) => (
                    <option key={t} value={t}>
                      {tierModel(t)}
                    </option>
                  ))}
                </select>
              </div>
              <input
                value={d.description ?? ""}
                onChange={(e) => editDraft(i, { description: e.target.value })}
                className="mt-2 w-full rounded-lg border border-border bg-panel px-2 py-1 text-xs text-fg-muted outline-none focus:border-accent"
              />
              <textarea
                value={d.system_prompt ?? ""}
                onChange={(e) => editDraft(i, { system_prompt: e.target.value })}
                rows={3}
                className="mt-2 w-full resize-none rounded-lg border border-border bg-panel px-2 py-1.5 text-xs outline-none focus:border-accent"
              />
              <div className="mt-2 flex justify-end">
                {saved.has(i) ? (
                  <span className="text-sm text-tier-easy">✓ Saved</span>
                ) : (
                  <Button variant="primary" size="sm" onClick={() => saveDraft(i)}>
                    Save agent
                  </Button>
                )}
              </div>
            </div>
          ))}
          {error && (
            <div className="rounded-lg bg-danger-subtle px-3 py-2 text-xs text-danger-fg">{error}</div>
          )}
        </div>
      )}
    </Dialog>
  );
}
