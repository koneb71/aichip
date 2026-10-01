import { useEffect, useState } from "react";
import { api, CheckCommand, ProjectChecks } from "../lib/api";

/**
 * The commands that decide whether an agent's work passes: `cargo test`,
 * `pnpm lint`, whatever this project already trusts.
 *
 * Saved with an explicit button rather than on every keystroke. These are
 * commands this machine will run, and a half-typed one should never be live.
 */
export function ChecksSettings({ projectId, fullAuto }: { projectId: string; fullAuto: boolean }) {
  const [draft, setDraft] = useState<ProjectChecks | null>(null);
  const [saved, setSaved] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .projectChecks(projectId)
      .then((c) => {
        setDraft(c);
        setSaved(JSON.stringify(c));
      })
      .catch((e) => setError(String(e)));
  }, [projectId]);

  if (!draft) return <p className="text-[11px] text-ink-dim">{error ?? "Loading…"}</p>;

  const dirty = JSON.stringify(draft) !== saved;
  const setCommand = (i: number, patch: Partial<CheckCommand>) =>
    setDraft({ ...draft, commands: draft.commands.map((c, j) => (j === i ? { ...c, ...patch } : c)) });

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      const next = await api.saveProjectChecks(projectId, {
        ...draft,
        commands: draft.commands.filter((c) => c.command.trim() !== ""),
      });
      setDraft(next);
      setSaved(JSON.stringify(next));
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  const input = "rounded-lg border border-line bg-panel px-2 py-1 text-xs outline-none focus:border-accent";

  return (
    <div className="space-y-2">
      {draft.commands.map((c, i) => (
        <div key={i} className="flex items-center gap-2">
          <input
            value={c.name}
            onChange={(e) => setCommand(i, { name: e.target.value })}
            placeholder="name"
            className={`${input} w-24`}
          />
          <input
            value={c.command}
            onChange={(e) => setCommand(i, { command: e.target.value })}
            placeholder="cargo test --workspace"
            className={`${input} min-w-0 flex-1 font-mono`}
          />
          <button
            onClick={() => setDraft({ ...draft, commands: draft.commands.filter((_, j) => j !== i) })}
            className="px-1 text-ink-dim hover:text-danger"
            title="Remove this check"
          >
            ✕
          </button>
        </div>
      ))}
      <button
        onClick={() => setDraft({ ...draft, commands: [...draft.commands, { name: "", command: "" }] })}
        className="text-xs text-ink-dim hover:text-ink"
      >
        + Add a check
      </button>

      <div className="flex flex-wrap items-center gap-3 text-xs text-ink-dim">
        <label className="flex items-center gap-1.5">
          Time limit
          <input
            type="number"
            min={10}
            max={3600}
            value={draft.timeoutSecs}
            onChange={(e) => setDraft({ ...draft, timeoutSecs: Number(e.target.value) })}
            className={`${input} w-20`}
          />
          s each
        </label>
        <label className="flex items-center gap-1.5">
          Fix failures by itself
          <select
            value={draft.autoFixAttempts}
            onChange={(e) => setDraft({ ...draft, autoFixAttempts: Number(e.target.value) })}
            className={input}
          >
            <option value={0}>never</option>
            <option value={1}>once</option>
            <option value={2}>up to twice</option>
            <option value={3}>up to 3 times</option>
          </select>
        </label>
        <button
          onClick={save}
          disabled={!dirty || busy}
          className="ml-auto rounded-lg bg-accent px-3 py-1 text-xs font-medium text-white disabled:opacity-40"
        >
          {busy ? "Saving…" : dirty ? "Save checks" : "Saved"}
        </button>
      </div>

      {error && <p className="rounded-lg bg-red-50 px-3 py-2 text-[11px] text-danger">{error}</p>}
      <p className="text-[11px] leading-relaxed text-ink-dim/80">
        They run in the card's worktree when an agent finishes. Because they execute code the agent may
        have just changed, they start by themselves — and fix failures by themselves — only after a{" "}
        <b>Full Auto</b> run, where the agent already had a shell; otherwise the card offers a Run checks
        button.
        {!fullAuto && " Full Auto is off for this project, so checks will always wait for your click."}
      </p>
    </div>
  );
}
