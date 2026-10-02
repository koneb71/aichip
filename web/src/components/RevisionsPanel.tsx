import { useEffect, useState } from "react";
import { History, RotateCcw } from "lucide-react";
import { api, ConfigRevision, RevisionKind } from "../lib/api";
import { Button } from "./ui/Button";
import { Dialog } from "./ui/Dialog";
import { EmptyState } from "./ui/Layout";
import { Badge } from "./ui/Badge";
import { toast } from "./ui/Toast";

/**
 * A setting's earlier versions, and putting one back.
 *
 * Each row says what restoring it would change against the setting as it is
 * now — "system_prompt, max_concurrent" — because "restore the version from
 * Tuesday" is only a decision when you can see what Tuesday had.
 */
export function HistoryButton({
  kind,
  id,
  label = "History",
  onRestored,
}: {
  kind: RevisionKind;
  id: string;
  label?: string;
  onRestored?: () => void;
}) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button size="sm" variant="ghost" icon={<History className="size-3.5" />} onClick={() => setOpen(true)}>
        {label}
      </Button>
      <Dialog open={open} onOpenChange={setOpen} title="Earlier versions" description="Restoring is an edit like any other: it is checked, recorded, and can itself be undone." width={560}>
        {open && <RevisionList kind={kind} id={id} onRestored={() => { setOpen(false); onRestored?.(); }} />}
      </Dialog>
    </>
  );
}

function RevisionList({ kind, id, onRestored }: { kind: RevisionKind; id: string; onRestored: () => void }) {
  const [revs, setRevs] = useState<ConfigRevision[] | null>(null);
  const [busy, setBusy] = useState<number | null>(null);

  useEffect(() => {
    api
      .configRevisions(kind, id)
      .then((r) => setRevs(r.revisions))
      .catch(() => setRevs([]));
  }, [kind, id]);

  if (revs === null) return <div className="skeleton h-32 rounded-lg" />;
  if (revs.length === 0) {
    return <EmptyState title="No earlier versions yet" hint="Each time this is saved, the version it replaced is kept here — the last 20." />;
  }

  const restore = async (rev: ConfigRevision) => {
    setBusy(rev.id);
    try {
      await api.restoreConfigRevision(rev.id);
      toast("Restored", { tone: "success", body: rev.changed.length ? `Changed back: ${rev.changed.join(", ")}` : undefined });
      onRestored();
    } catch (e) {
      toast("Could not restore it", { tone: "danger", body: String(e).replace(/^Error:\s*/, "") });
    } finally {
      setBusy(null);
    }
  };

  return (
    <ul className="divide-y divide-border">
      {revs.map((r) => (
        <li key={r.id} className="flex items-start gap-3 py-2.5">
          <div className="min-w-0 flex-1">
            <div className="tabular text-[13px] text-fg">{new Date(r.createdAt).toLocaleString()}</div>
            <div className="mt-1 flex flex-wrap gap-1">
              {r.changed.length === 0 ? (
                <span className="text-xs text-fg-subtle">Same as now</span>
              ) : (
                r.changed.map((k) => (
                  <Badge key={k} tone="neutral" className="font-mono">
                    {k}
                  </Badge>
                ))
              )}
            </div>
          </div>
          <Button
            size="sm"
            icon={<RotateCcw className="size-3.5" />}
            loading={busy === r.id}
            disabled={busy !== null || r.changed.length === 0}
            onClick={() => void restore(r)}
          >
            Restore
          </Button>
        </li>
      ))}
    </ul>
  );
}
