import { useCallback, useEffect, useRef, useState } from "react";
import { Download, Globe, Bot, Cpu } from "lucide-react";
import { api, AuditEntry, AuditQuery } from "../lib/api";
import { Page } from "../components/ui/Surface";
import { EmptyState, PageHeader, Table, Toolbar } from "../components/ui/Layout";
import { Badge, type Tone } from "../components/ui/Badge";
import { Button, buttonClasses } from "../components/ui/Button";
import { Select } from "../components/ui/Field";

/**
 * The ledger: what was done, by what, to what. Append-only on the server.
 *
 * "API" is honest rather than flattering: aichip has no login, so a request
 * through the dashboard's API is anything on this machine that made one.
 */
export default function AuditPage() {
  const [filter, setFilter] = useState<AuditQuery>({});
  const [entries, setEntries] = useState<AuditEntry[] | null>(null);
  const [next, setNext] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  // Which filter's answer is wanted. A slower reply to an earlier filter (or
  // to "load more" under it) lands after the change and is dropped.
  const seq = useRef(0);

  const load = useCallback(
    async (more: boolean) => {
      const mine = more ? seq.current : ++seq.current;
      setBusy(true);
      try {
        const r = await api.audit({ ...filter, limit: 100, before: more ? next ?? undefined : undefined });
        if (mine !== seq.current) return;
        setEntries((prev) => (more && prev ? [...prev, ...r.entries] : r.entries));
        setNext(r.entries.length >= 100 ? r.next : null);
      } catch {
        setEntries((prev) => prev ?? []);
      } finally {
        setBusy(false);
      }
    },
    [filter, next],
  );

  // Reload from the top whenever the filter changes.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => void load(false), [filter]);

  return (
    <Page wide>
      <div className="mx-auto max-w-6xl">
        <PageHeader
          title="Audit log"
          description="Every change made through the API, every tool an agent called, and what aichip did on its own. Agent and system entries are kept 90 days; API entries for good."
          actions={
            <a href={api.auditCsvUrl(filter)} download className={buttonClasses({ size: "sm" })}>
              <Download className="size-3.5" />
              Export CSV
            </a>
          }
        />
        <Toolbar>
          <Select
            className="w-44"
            value={filter.actorKind ?? ""}
            onChange={(e) => setFilter((f) => ({ ...f, actorKind: e.target.value || undefined }))}
            aria-label="Who"
          >
            <option value="">Everyone</option>
            <option value="api">Through the API</option>
            <option value="agent">Agents</option>
            <option value="system">aichip itself</option>
          </Select>
          <Select
            className="w-44"
            value={filter.entityKind ?? ""}
            onChange={(e) => setFilter((f) => ({ ...f, entityKind: e.target.value || undefined, entityId: undefined }))}
            aria-label="What"
          >
            <option value="">Anything</option>
            {["tasks", "runs", "agents", "teams", "routines", "budgets", "projects", "skills", "inbox", "settings"].map((k) => (
              <option key={k} value={k}>
                {k}
              </option>
            ))}
          </Select>
        </Toolbar>

        {entries === null ? (
          <div className="skeleton h-64 rounded-lg" />
        ) : entries.length === 0 ? (
          <EmptyState title="Nothing recorded yet" hint="Changes, agent tool calls and automatic steps show up here as they happen." />
        ) : (
          <Table>
            <thead>
              <tr>
                <th className="w-40">When</th>
                <th className="w-32">Who</th>
                <th>What</th>
                <th className="w-48">On</th>
              </tr>
            </thead>
            <tbody>
              {entries.map((e) => (
                <tr key={e.id}>
                  <td className="tabular whitespace-nowrap text-xs text-fg-muted">{new Date(e.at).toLocaleString()}</td>
                  <td>
                    <Actor e={e} />
                  </td>
                  <td className="max-w-0">
                    <div className="truncate text-[13px]" title={e.summary || e.action}>
                      {e.summary || e.action}
                    </div>
                  </td>
                  <td className="max-w-0">
                    {e.entityKind && (
                      <span className="block truncate font-mono text-[11px] text-fg-muted" title={`${e.entityKind} ${e.entityId ?? ""}`}>
                        {e.entityKind}
                        {e.entityId ? ` · ${e.entityId.slice(0, 8)}` : ""}
                      </span>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </Table>
        )}
        {next !== null && (
          <div className="mt-3 flex justify-center">
            <Button size="sm" variant="ghost" loading={busy} onClick={() => void load(true)}>
              Older entries
            </Button>
          </div>
        )}
      </div>
    </Page>
  );
}

const ACTOR: Record<AuditEntry["actorKind"], { label: string; tone: Tone; icon: React.ReactNode }> = {
  api: { label: "API", tone: "neutral", icon: <Globe className="size-3" /> },
  agent: { label: "Agent", tone: "complex", icon: <Bot className="size-3" /> },
  system: { label: "aichip", tone: "info", icon: <Cpu className="size-3" /> },
};

function Actor({ e }: { e: AuditEntry }) {
  const a = ACTOR[e.actorKind];
  return (
    <Badge tone={a.tone} icon={a.icon} title={e.actorRunId ? `run ${e.actorRunId}` : undefined}>
      {a.label}
    </Badge>
  );
}
