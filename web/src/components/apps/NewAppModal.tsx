import { useState } from "react";
import { api, type AppRuntime } from "../../lib/api";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { Checkbox, Input, Textarea } from "../ui/Field";
import { runtimeBlurb, starterManifest } from "../../lib/apps";

const RUNTIMES: { id: AppRuntime; label: string }[] = [
  { id: "module", label: "Module" },
  { id: "node", label: "Node" },
  { id: "static", label: "Static" },
];

export function NewAppModal({
  workspaceId,
  onClose,
  onInstalled,
}: {
  workspaceId: string;
  onClose: () => void;
  onInstalled: () => void;
}) {
  const [runtime, setRuntime] = useState<AppRuntime>("module");
  const [manifest, setManifest] = useState(() => starterManifest("module"));
  const [brief, setBrief] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [writing, setWriting] = useState(false);
  const [polish, setPolish] = useState(false);

  /**
   * Switching runtime replaces the manifest, because the two starters declare
   * different things and a container app's `views:` block would be refused.
   * An edited manifest is kept rather than silently thrown away — losing typing
   * to a radio button is worse than an inconsistent example.
   */
  const chooseRuntime = (next: AppRuntime) => {
    setRuntime(next);
    setManifest((current) =>
      current === starterManifest(runtime) ? starterManifest(next) : current,
    );
  };

  /**
   * Ask an agent for a manifest, and put it in the box rather than installing
   * it. Reading the thing before it becomes real is the whole reason an app is
   * a declaration instead of code.
   */
  const generate = async () => {
    if (!brief.trim()) {
      setError("Say what the app is for first.");
      return;
    }
    setWriting(true);
    setError(null);
    try {
      const r = await api.generateApp(brief.trim(), runtime);
      setManifest(r.manifest);
      // A manifest that does not parse still lands in the box, with the
      // parser's complaint above it: the fix is usually one line, and
      // throwing the draft away to regenerate costs another call.
      setError(r.error);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setWriting(false);
    }
  };

  const install = async () => {
    setBusy(true);
    setError(null);
    try {
      const app = await api.installApp(workspaceId, manifest, brief);
      // The scaffolded screens already work; this sends an agent to make them
      // *good*. Fired after install so a failed run costs nothing but the run —
      // the app is installed either way, and the card lands like any other
      // change. Best-effort: an error here is the card's problem, not the
      // install's.
      if (polish && app.runtime !== "module" && brief.trim()) {
        await api.changeApp(app.id, brief.trim()).catch(() => {});
      }
      onInstalled();
    } catch (e) {
      // The parser names the key — "models.expense.fields.qty: unknown field
      // type" — so it is shown as it came rather than summarised.
      setError(String(e).replace(/^Error:\s*/, ""));
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
      title="New app"
      description="Every app declares models, which become real tables. What draws it is the runtime."
      width={672}
      footer={
        <>
          <Button variant="ghost" size="sm" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" size="sm" onClick={install} disabled={busy}>
            {busy ? "Installing…" : "Install"}
          </Button>
        </>
      }
    >
      <div className="flex flex-col">
        <div className="flex items-center gap-1">
          {RUNTIMES.map((r) => (
            <button
              key={r.id}
              type="button"
              onClick={() => chooseRuntime(r.id)}
              className={
                "ring-focus rounded-lg border px-2.5 py-1 text-xs " +
                (runtime === r.id
                  ? "border-accent bg-accent-subtle font-medium text-fg"
                  : "border-border text-fg-muted hover:bg-panel-2")
              }
            >
              {r.label}
            </button>
          ))}
          {/* Said at the moment of choosing rather than on the failed build:
              a container app on a machine without Docker installs fine and
              then never runs, which looks like a bug in the app. */}
          <span className="ml-2 min-w-0 flex-1 truncate text-[11px] text-fg-muted">
            {runtimeBlurb(runtime)}
          </span>
        </div>

        <div className="mt-4">
          <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-fg-muted">
            What is it for
          </span>
          <div className="flex gap-2">
            <Input
              value={brief}
              onChange={(e) => setBrief(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !writing) generate();
              }}
              placeholder="track my spending by category"
              className="flex-1 text-sm!"
            />
            <Button onClick={generate} disabled={writing || busy}>
              {writing ? "Writing…" : "Write it for me"}
            </Button>
          </div>
        </div>

        <label className="mt-3 flex min-h-0 flex-1 flex-col">
          <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-fg-muted">
            Manifest
          </span>
          <Textarea
            value={manifest}
            onChange={(e) => setManifest(e.target.value)}
            spellCheck={false}
            className="min-h-[18rem]! flex-1 resize-none p-3! font-mono text-xs!"
          />
        </label>

        {error && (
          <div className="mt-3 rounded-lg bg-danger-subtle px-3 py-2 font-mono text-[11px] text-danger-fg">
            {error}
          </div>
        )}

        {runtime !== "module" && (
          <Checkbox
            checked={polish}
            onChange={setPolish}
            label="Have an agent polish the screens after install (costs a run)"
            className="mt-3 text-xs!"
          />
        )}
      </div>
    </Dialog>
  );
}
