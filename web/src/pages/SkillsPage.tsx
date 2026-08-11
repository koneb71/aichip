import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { api, Skill } from "../lib/api";
import { useWorkspace } from "../lib/workspace";
import { Card, Empty, Item, Page, PageHead, Stagger, TintIcon } from "../components/ui/Surface";
import { Icon } from "../components/ui/Icon";
import { tappable } from "../lib/motion";

/**
 * Skills: a named way of doing something, smaller than an agent.
 *
 * An agent is *who* does the work. A skill is *how* one job is done here — the
 * release checklist, how migrations get written, what a bug report must
 * contain. They compose: pick both on a card.
 *
 * A skill applies only when you name it, never because something matched its
 * description. That is the whole reason it cannot go wrong quietly: if a run
 * behaves oddly, the cause is a name you typed, not an invisible list of
 * everything switched on.
 */
export default function SkillsPage() {
  const { active } = useWorkspace();
  const [skills, setSkills] = useState<Skill[]>([]);
  const [editing, setEditing] = useState<Skill | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    if (!active) return;
    api.skills(active.id).then((r) => setSkills(r.skills)).catch(() => {});
  }, [active]);
  useEffect(load, [load]);

  const add = async () => {
    if (!active) return;
    setError(null);
    try {
      const s = await api.createSkill({
        workspace_id: active.id,
        name: `new-skill-${skills.length + 1}`,
        description: "",
        instructions: "",
        must_not: "",
      });
      load();
      setEditing(s);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    }
  };

  return (
    <Page>
      <PageHead
        title="Skills"
        subtitle={
          <>
            A named way of doing something. An agent is <em>who</em> does the work; a skill is{" "}
            <em>how</em> this kind of job is done here — and they compose. A skill applies only
            when you name it: <code className="rounded bg-panel-2 px-1 font-mono text-[11px]">@its-name</code>{" "}
            in chat, or picked on a card. Never because it looked relevant, which is what stops a
            stale one steering a request that never mentioned it.
          </>
        }
        actions={
          <motion.button
            {...tappable}
            onClick={add}
            className="ring-focus flex shrink-0 items-center gap-1.5 rounded-xl bg-accent px-3.5 py-2 text-sm font-semibold text-white shadow-[0_2px_10px_-2px_var(--color-accent)] transition-[filter] hover:brightness-110"
          >
            <Icon name="plus" size={15} strokeWidth={2.5} />
            New skill
          </motion.button>
        }
      />

      {error && (
        <div className="mb-4 max-w-xl rounded-xl bg-red-50 px-3.5 py-2.5 text-xs text-danger">
          {error}
        </div>
      )}

      <Stagger className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {skills.map((s) => (
          <Item key={s.id}>
            <Card onClick={() => setEditing(s)} className="h-full p-4">
              <div className="flex items-center gap-2.5">
                <TintIcon tint={s.enabled ? "amber" : "slate"} size={34}>
                  <Icon name="skills" size={16} />
                </TintIcon>
                <div className="flex min-w-0 items-baseline gap-2">
                  <span className="min-w-0 truncate font-mono text-sm font-semibold">
                    @{s.name}
                  </span>
                  {!s.enabled && (
                    <span className="shrink-0 rounded-full bg-panel-2 px-2 py-0.5 text-[10px] text-ink-dim">
                      off
                    </span>
                  )}
                </div>
              </div>
              <p className="mt-3 line-clamp-2 text-xs leading-relaxed text-ink-dim">
                {s.description || "no description yet"}
              </p>
              {s.mustNot.trim() && (
                <p className="mt-2 line-clamp-1 rounded-lg bg-amber-50 px-2 py-1 text-[11px] text-amber-700">
                  won't: {s.mustNot}
                </p>
              )}
            </Card>
          </Item>
        ))}
        {skills.length === 0 && (
          <div className="sm:col-span-2 lg:col-span-3">
            <Empty
              icon={<Icon name="skills" size={28} />}
              title="No skills yet"
              hint={'A good first one is narrow — "how we write a migration", not "how we code".'}
            />
          </div>
        )}
      </Stagger>

      <AnimatePresence>
        {editing && (
          <SkillEditor
            skill={editing}
            onClose={() => setEditing(null)}
            onChanged={(s) => {
              setEditing(s);
              load();
            }}
            onDeleted={() => {
              setEditing(null);
              load();
            }}
          />
        )}
      </AnimatePresence>
    </Page>
  );
}

function SkillEditor({
  skill,
  onClose,
  onChanged,
  onDeleted,
}: {
  skill: Skill;
  onClose: () => void;
  onChanged: (s: Skill) => void;
  onDeleted: () => void;
}) {
  const [draft, setDraft] = useState(skill);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [trying, setTrying] = useState(false);
  const [tryPrompt, setTryPrompt] = useState("");
  const [result, setResult] = useState<{ output: string; prompt: string } | null>(null);

  const save = async (patch: Partial<Skill> = {}) => {
    setBusy(true);
    setError(null);
    try {
      const next = { ...draft, ...patch };
      const saved = await api.updateSkill(skill.id, {
        name: next.name,
        description: next.description,
        instructions: next.instructions,
        must_not: next.mustNot,
        enabled: next.enabled,
      });
      setDraft(saved);
      onChanged(saved);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  const runTry = async () => {
    setTrying(true);
    setError(null);
    setResult(null);
    try {
      setResult(await api.trySkill(skill.id, tryPrompt));
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setTrying(false);
    }
  };

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={busy ? undefined : onClose}
      className="fixed inset-0 z-40 flex items-start justify-center overflow-y-auto bg-black/25 backdrop-blur-[3px] p-4"
    >
      <motion.div
        initial={{ scale: 0.97, y: 12, opacity: 0 }}
        animate={{ scale: 1, y: 0, opacity: 1 }}
        transition={{ type: "spring", stiffness: 220, damping: 26 }}
        exit={{ scale: 0.97, y: 8 }}
        onClick={(e) => e.stopPropagation()}
        className="card-shadow-lg my-8 w-full max-w-2xl rounded-2xl border border-line bg-panel p-5"
      >
        <div className="flex items-start justify-between gap-3">
          <h3 className="text-sm font-semibold">Skill</h3>
          <label className="flex shrink-0 items-center gap-2 text-xs">
            <input
              type="checkbox"
              checked={draft.enabled}
              disabled={busy}
              onChange={(e) => save({ enabled: e.target.checked })}
            />
            <span className={draft.enabled ? "text-ink" : "text-ink-dim"}>
              {draft.enabled ? "In use" : "Off"}
            </span>
          </label>
        </div>

        <Field label="Name" hint="What you type after @. Shares one namespace with your agents.">
          <input
            value={draft.name}
            onChange={(e) => setDraft({ ...draft, name: e.target.value })}
            onBlur={() => draft.name !== skill.name && save()}
            disabled={busy}
            className="w-full rounded-lg border border-line bg-surface px-2 py-1.5 font-mono text-sm outline-none focus:border-accent"
          />
        </Field>

        <Field label="When to use it" hint="One line. This is all you see in the picker.">
          <input
            value={draft.description}
            onChange={(e) => setDraft({ ...draft, description: e.target.value })}
            onBlur={() => save()}
            disabled={busy}
            placeholder="how we cut a release"
            className="w-full rounded-lg border border-line bg-surface px-2 py-1.5 text-sm outline-none focus:border-accent"
          />
        </Field>

        <Field label="How to do it">
          <textarea
            value={draft.instructions}
            onChange={(e) => setDraft({ ...draft, instructions: e.target.value })}
            onBlur={() => save()}
            disabled={busy}
            rows={8}
            placeholder={"Name the steps, in order.\nSay what the finished thing looks like.\nSay when to stop and ask."}
            className="w-full resize-y rounded-lg border border-line bg-surface p-2 font-mono text-xs leading-relaxed outline-none focus:border-accent"
          />
        </Field>

        {/* Its own field rather than a paragraph in the one above, because
            "explicit about what it should not do" is the part free text always
            omits — a box asks the question. */}
        <Field
          label="What it must not do"
          hint="Kept separate and put last in the prompt, where it is read rather than skimmed."
        >
          <textarea
            value={draft.mustNot}
            onChange={(e) => setDraft({ ...draft, mustNot: e.target.value })}
            onBlur={() => save()}
            disabled={busy}
            rows={3}
            placeholder="never force-push; never edit files outside src/"
            className="w-full resize-y rounded-lg border border-line bg-surface p-2 font-mono text-xs leading-relaxed outline-none focus:border-accent"
          />
        </Field>

        <div className="mt-5 rounded-xl border border-line bg-surface p-3">
          <div className="text-xs font-medium">Try it</div>
          <p className="mt-0.5 text-[11px] leading-relaxed text-ink-dim">
            Runs the skill against one harmless prompt, with{" "}
            <span className="font-medium text-ink">no tools, no repository and no worktree</span> —
            so whatever it says to do, there is nothing here to do it to. This tells you how
            the skill reads, not what it would do to your files.
          </p>
          <div className="mt-2 flex gap-2">
            <input
              value={tryPrompt}
              onChange={(e) => setTryPrompt(e.target.value)}
              placeholder="Describe what you would do for: bump the version to 2.1"
              className="min-w-0 flex-1 rounded-lg border border-line bg-panel px-2 py-1.5 text-xs outline-none focus:border-accent"
            />
            <button
              onClick={runTry}
              disabled={trying || !tryPrompt.trim()}
              className="shrink-0 rounded-lg border border-line px-2.5 py-1.5 text-xs hover:border-ink-dim disabled:opacity-40"
            >
              {trying ? "Trying…" : "Try it"}
            </button>
          </div>
          {result && (
            <div className="mt-3">
              <pre className="max-h-56 overflow-y-auto whitespace-pre-wrap rounded-lg bg-panel p-2 text-[11px] leading-relaxed">
                {result.output}
              </pre>
              <details className="mt-1.5">
                {/* Half of what a test tells you is whether the skill says what
                    you thought it said. */}
                <summary className="cursor-pointer text-[11px] text-ink-dim">
                  what it was actually sent
                </summary>
                <pre className="mt-1 max-h-56 overflow-y-auto whitespace-pre-wrap rounded-lg bg-panel p-2 font-mono text-[10px] text-ink-dim">
                  {result.prompt}
                </pre>
              </details>
            </div>
          )}
        </div>

        {error && (
          <div className="mt-3 whitespace-pre-wrap rounded-lg bg-red-50 px-3 py-2 text-[11px] leading-relaxed text-danger">
            {error}
          </div>
        )}

        <div className="mt-4 flex items-center gap-2">
          <button onClick={onClose} className="rounded-lg px-3 py-1.5 text-xs text-ink-dim">
            Done
          </button>
          <button
            onClick={async () => {
              await api.deleteSkill(skill.id);
              onDeleted();
            }}
            className="ml-auto rounded-lg border border-line px-3 py-1.5 text-xs text-ink-dim hover:border-danger hover:text-danger"
          >
            Delete
          </button>
        </div>

        <p className="mt-3 text-[11px] leading-relaxed text-ink-dim">
          <span className="font-medium text-ink">No secrets here.</span> This text goes into
          a prompt, so a save containing something key-shaped is refused.
        </p>
      </motion.div>
    </motion.div>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="mt-4">
      <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
        {label}
      </span>
      {children}
      {hint && <p className="mt-1 text-[11px] text-ink-dim/80">{hint}</p>}
    </div>
  );
}
