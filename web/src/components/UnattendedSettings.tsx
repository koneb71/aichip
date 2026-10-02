import { useEffect, useState } from "react";
import { api, Unattended } from "../lib/api";
import { HistoryButton } from "./RevisionsPanel";
import { Select, Switch } from "./ui/Field";
import { toast } from "./ui/Toast";

/**
 * Runs that stop showing signs of life.
 *
 * A run whose process is gone is always marked failed — nothing else could
 * ever finish it. Stopping a run for being quiet is a person's call (a long
 * test suite is quiet too), and so is restarting one by itself.
 */
export function UnattendedSettings() {
  const [v, setV] = useState<Unattended | null>(null);
  const [loads, setLoads] = useState(0);

  useEffect(() => {
    api
      .unattended()
      .then((r) => setV(r.unattended))
      .catch(() => setV({ silenceMinutes: 0, autoResume: false }));
  }, [loads]);

  if (!v) return null;

  const save = async (patch: Partial<Unattended>) => {
    const next = { ...v, ...patch };
    setV(next);
    try {
      const r = await api.saveUnattended(next);
      setV(r.unattended);
    } catch (e) {
      toast("Not saved", { tone: "danger", body: String(e).replace(/^Error:\s*/, "") });
      setLoads((n) => n + 1);
    }
  };

  return (
    <section className="mt-8 max-w-2xl rounded-xl border border-border bg-panel p-4">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="text-sm font-semibold text-fg">Unattended runs</h2>
          <p className="mt-0.5 text-xs leading-relaxed text-fg-muted">
            A run nothing is executing any more is always marked failed and said so on its card. The rest is up to you.
          </p>
        </div>
        <HistoryButton kind="unattended" id="unattended" onRestored={() => setLoads((n) => n + 1)} />
      </div>
      <div className="mt-3 space-y-3">
        <label className="flex flex-wrap items-center gap-2 text-[13px] text-fg">
          Stop a run that has said nothing for
          <Select
            className="w-36"
            aria-label="Silence limit"
            value={v.silenceMinutes}
            onChange={(e) => void save({ silenceMinutes: Number(e.target.value) })}
          >
            <option value={0}>never</option>
            <option value={15}>15 minutes</option>
            <option value={30}>30 minutes</option>
            <option value={60}>an hour</option>
            <option value={120}>2 hours</option>
          </Select>
        </label>
        <p className="-mt-2 text-xs text-fg-muted">Never while it waits on you for a permission.</p>
        <div className="flex items-start gap-3">
          <Switch checked={v.autoResume} onChange={(on) => void save({ autoResume: on })} label="Resume stopped runs by themselves" />
          <div>
            <div className="text-[13px] text-fg">Resume stopped runs by themselves</div>
            <div className="text-xs leading-relaxed text-fg-muted">
              Through the Resume button's own checks — agent, budget, worktree — and at most twice along one chain.
              Each resume is a run, and costs like one.
            </div>
          </div>
        </div>
      </div>
    </section>
  );
}
