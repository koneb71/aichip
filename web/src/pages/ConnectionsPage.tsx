import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { api, GitHubConnect, GitHubStatus, McpServer, McpTestResult } from "../lib/api";
import { useWorkspace } from "../lib/workspace";

/**
 * MCP servers the user connects.
 *
 * Every agent could previously do exactly three things — read files, write
 * files, run bash — because the only MCP server in play was aichip's own.
 * This is where that stops being the ceiling: connect a browser, a database,
 * an issue tracker, then tick it on for the agents that should have it.
 *
 * Nothing here touches credentials. aichip spawns the official CLI and hands
 * it a `--mcp-config` file, which is the same thing you'd write by hand.
 */
/**
 * GitHub, which is a connection but not an MCP server.
 *
 * It sits above the server list rather than in it because there is nothing to
 * configure — aichip drives the `gh` CLI you already have, so the only question
 * is whether it is installed and logged in. There is no field to fill in and no
 * token to paste, which is the point.
 *
 * Re-checked on every visit, because `gh auth login` happens in a terminal
 * while aichip is running, and telling someone to go and run it is most of what
 * this card is for.
 */
function GitHubCard() {
  const [state, setState] = useState<GitHubStatus | null>(null);
  const [flow, setFlow] = useState<GitHubConnect | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const refresh = useCallback(
    () => api.github().then(setState).catch(() => {}),
    [],
  );
  useEffect(() => {
    refresh();
  }, [refresh]);

  // Poll only while a flow is open. It finishes when the person finishes it in
  // their browser, which nothing here can hurry along.
  useEffect(() => {
    if (!flow) return;
    const t = setInterval(async () => {
      const p = await api.githubConnectStatus(flow.id).catch(() => null);
      if (!p || p.state === "waiting") return;
      clearInterval(t);
      setFlow(null);
      if (p.state === "failed") setError(p.reason);
      else refresh();
    }, 2000);
    return () => clearInterval(t);
  }, [flow, refresh]);

  const connect = async () => {
    setBusy(true);
    setError(null);
    try {
      setFlow(await api.connectGitHub());
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  if (!state) return null;

  const account = state.accounts.find((a) => a.active) ?? state.accounts[0];
  const problem = account?.problem;

  return (
    <div className="mt-6 max-w-4xl rounded-xl border border-line bg-panel p-4">
      <div className="flex flex-wrap items-baseline gap-2">
        <span className="text-sm font-semibold">GitHub</span>
        {state.usable ? (
          <span className="rounded-full bg-tier-easy-soft px-2 py-0.5 text-[11px] text-tier-easy">
            ✓ {account?.login} on {account?.host}
          </span>
        ) : (
          <span className="rounded-full bg-panel-2 px-2 py-0.5 text-[11px] text-ink-dim">
            {state.installed ? "not logged in" : "gh not installed"}
          </span>
        )}
      </div>

      <p className="mt-1.5 max-w-xl text-xs text-ink-dim">
        {state.usable
          ? "Clone a repo, open a pull request from a finished task, and pull issues in as cards. aichip runs your own gh CLI and never sees a token."
          : state.installed
            ? "aichip drives the gh CLI you already have, so there is no token to paste here — it just needs to be logged in."
            : "Install the GitHub CLI and log in, and cloning, pull requests and issue import become available. aichip never handles a token of its own."}
      </p>

      {/* `gh`'s own words. "Not logged in" alone would send someone to re-auth
          without saying that their token was revoked rather than missing. */}
      {problem && (
        <p className="mt-1.5 text-xs text-danger">
          {account?.login} on {account?.host}: {problem}
        </p>
      )}

      {error && (
        <p className="mt-1.5 text-xs text-danger">{error}</p>
      )}

      {/* Nothing to offer without the binary: this is a package to install,
          not a button to press. */}
      {!state.usable && !state.installed && (
        <code className="mt-2 inline-block rounded-md bg-panel-2 px-2 py-1 text-[11px]">
          brew install gh
        </code>
      )}

      {!state.usable && state.installed && !flow && (
        <button
          onClick={connect}
          disabled={busy}
          className="mt-2 rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white disabled:opacity-50"
        >
          {busy ? "Starting…" : "Connect GitHub"}
        </button>
      )}

      {flow && (
        <div className="mt-2 rounded-xl border border-line bg-panel-2 p-3">
          <div className="text-xs text-ink-dim">
            Enter this code on GitHub. aichip never sees the token — GitHub
            gives it straight to your <code className="text-[11px]">gh</code>.
          </div>
          <div className="mt-2 flex flex-wrap items-center gap-2">
            <code className="rounded-md bg-panel px-2.5 py-1.5 font-mono text-sm tracking-widest">
              {flow.code}
            </code>
            <button
              onClick={() => {
                navigator.clipboard?.writeText(flow.code);
                setCopied(true);
              }}
              className="rounded-lg border border-line px-2 py-1 text-[11px] hover:bg-line/40"
            >
              {copied ? "copied" : "copy"}
            </button>
            <a
              href={flow.url}
              target="_blank"
              rel="noreferrer"
              className="rounded-lg bg-accent px-2.5 py-1 text-[11px] font-medium text-white"
            >
              Open GitHub
            </a>
            <button
              onClick={() => {
                api.cancelGitHubConnect(flow.id);
                setFlow(null);
              }}
              className="text-[11px] text-ink-dim hover:text-ink"
            >
              cancel
            </button>
          </div>
          <div className="mt-1.5 text-[11px] text-ink-dim">
            Waiting for you to finish in the browser…
          </div>
        </div>
      )}
    </div>
  );
}

export default function ConnectionsPage() {
  const { active } = useWorkspace();
  const [servers, setServers] = useState<McpServer[]>([]);
  const [editing, setEditing] = useState<McpServer | "new" | null>(null);

  const load = useCallback(() => {
    if (!active) return;
    api.mcpServers(active.id).then((r) => setServers(r.servers)).catch(() => {});
  }, [active]);

  useEffect(load, [load]);

  return (
    <div className="h-full overflow-y-auto p-8">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Connections</h1>
          <p className="mt-1 max-w-xl text-sm text-ink-dim">
            MCP servers give your agents tools beyond reading, writing, and running
            commands. Connect one here, then switch it on for the agents that should
            have it.
          </p>
        </div>
        <button
          onClick={() => setEditing("new")}
          className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white"
        >
          + Connect a server
        </button>
      </div>

      <GitHubCard />

      <div className="mt-6 grid max-w-4xl gap-3">
        {servers.map((s) => (
          <ServerCard key={s.id} server={s} onEdit={() => setEditing(s)} onChanged={load} />
        ))}
        {servers.length === 0 && (
          <div className="rounded-xl border border-dashed border-line p-8 text-center">
            <div className="text-sm text-ink-dim">Nothing connected yet.</div>
            <div className="mx-auto mt-3 max-w-md text-left text-xs text-ink-dim">
              A few that work well:
              <ul className="mt-2 space-y-1.5">
                <li>
                  <span className="font-medium text-ink">Playwright</span> —{" "}
                  <code className="rounded bg-panel-2 px-1">
                    npx -y @playwright/mcp
                  </code>{" "}
                  lets a QA agent actually open the page it's testing.
                </li>
                <li>
                  <span className="font-medium text-ink">Postgres</span> — a read-only
                  connection so an agent designs against the real schema instead of
                  guessing from migrations.
                </li>
                <li>
                  <span className="font-medium text-ink">Your issue tracker</span> — so
                  a task can read the ticket it's implementing.
                </li>
              </ul>
            </div>
          </div>
        )}
      </div>

      <AnimatePresence>
        {editing && (
          <ServerEditor
            workspaceId={active?.id ?? ""}
            server={editing === "new" ? null : editing}
            onClose={() => setEditing(null)}
            onSaved={() => {
              setEditing(null);
              load();
            }}
          />
        )}
      </AnimatePresence>
    </div>
  );
}

function ServerCard({
  server,
  onEdit,
  onChanged,
}: {
  server: McpServer;
  onEdit: () => void;
  onChanged: () => void;
}) {
  const [test, setTest] = useState<McpTestResult | null>(null);
  const [testing, setTesting] = useState(false);

  const runTest = async () => {
    setTesting(true);
    setTest(null);
    try {
      setTest(await api.testMcpServer(server.id));
    } catch (e) {
      setTest({ ok: false, error: String(e).replace(/^Error:\s*/, "") });
    } finally {
      setTesting(false);
    }
  };

  return (
    <motion.div
      layout
      className="card-shadow rounded-xl border border-line bg-panel p-4"
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <span className="text-sm font-semibold">{server.name}</span>
            <span className="rounded-full bg-panel-2 px-2 py-0.5 text-[11px] text-ink-dim">
              {server.transport}
            </span>
            {!server.enabled && (
              <span className="rounded-full bg-panel-2 px-2 py-0.5 text-[11px] text-ink-dim">
                off
              </span>
            )}
          </div>
          <div className="mt-1 truncate font-mono text-xs text-ink-dim">
            {server.transport === "stdio"
              ? [server.command, ...server.args].join(" ")
              : server.url}
          </div>
          <div className="mt-1 text-[11px] text-ink-dim">
            Tools appear to agents as{" "}
            <code className="rounded bg-panel-2 px-1">{server.toolPrefix}__*</code>
          </div>
        </div>
        <div className="flex shrink-0 gap-2">
          <button
            onClick={runTest}
            disabled={testing}
            className="rounded-lg border border-line px-3 py-1 text-xs hover:bg-panel-2 disabled:opacity-50"
          >
            {testing ? "Connecting…" : "Test"}
          </button>
          <button
            onClick={onEdit}
            className="rounded-lg border border-line px-3 py-1 text-xs hover:bg-panel-2"
          >
            Edit
          </button>
          <button
            onClick={async () => {
              await api.deleteMcpServer(server.id);
              onChanged();
            }}
            className="rounded-lg border border-line px-3 py-1 text-xs text-ink-dim hover:border-danger hover:text-danger"
          >
            Remove
          </button>
        </div>
      </div>

      {test && (
        <div
          className={`mt-3 rounded-lg px-3 py-2 text-xs ${
            test.ok
              ? "bg-tier-easy-soft text-tier-easy"
              : "bg-red-50 text-danger"
          }`}
        >
          {test.ok ? (
            test.tools.length > 0 ? (
              <>
                <span className="font-medium">
                  Connected — {test.tools.length} tool
                  {test.tools.length === 1 ? "" : "s"}:
                </span>{" "}
                {test.tools.join(", ")}
              </>
            ) : (
              <span className="font-medium">Connected.</span>
            )
          ) : (
            test.error
          )}
        </div>
      )}
    </motion.div>
  );
}

function ServerEditor({
  workspaceId,
  server,
  onClose,
  onSaved,
}: {
  workspaceId: string;
  server: McpServer | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [name, setName] = useState(server?.name ?? "");
  const [transport, setTransport] = useState(server?.transport ?? "stdio");
  // Edited as one line because that's how these are documented and copied.
  const [command, setCommand] = useState(
    server ? [server.command, ...server.args].filter(Boolean).join(" ") : "",
  );
  const [url, setUrl] = useState(server?.url ?? "");
  const [env, setEnv] = useState(
    Object.entries(server?.env ?? {})
      .map(([k, v]) => `${k}=${v}`)
      .join("\n"),
  );
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      const [cmd, ...args] = command.trim().split(/\s+/).filter(Boolean);
      const body = {
        workspace_id: workspaceId,
        name,
        transport,
        command: cmd ?? null,
        args,
        url: url.trim() || null,
        env: parseEnv(env),
      };
      if (server) await api.updateMcpServer(server.id, body);
      else await api.createMcpServer(body);
      onSaved();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={onClose}
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 p-4"
    >
      <motion.div
        initial={{ y: 20, scale: 0.98 }}
        animate={{ y: 0, scale: 1 }}
        exit={{ y: 20, scale: 0.98 }}
        transition={{ type: "spring", stiffness: 380, damping: 30 }}
        onClick={(e) => e.stopPropagation()}
        className="card-shadow max-h-[90vh] w-full max-w-lg overflow-y-auto rounded-2xl border border-line bg-panel p-6"
      >
        <div className="text-base font-semibold">
          {server ? `Edit ${server.name}` : "Connect an MCP server"}
        </div>

        <label className="mt-4 block text-xs font-medium text-ink-dim">Name</label>
        <input
          autoFocus
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="playwright"
          className="mt-1 w-full rounded-lg border border-line bg-panel px-3 py-2 text-sm outline-none focus:border-accent"
        />
        <div className="mt-1 text-[11px] text-ink-dim">
          Becomes the tool prefix agents see. Spaces and punctuation become
          underscores.
        </div>

        <label className="mt-4 block text-xs font-medium text-ink-dim">How it runs</label>
        <div className="mt-1 flex gap-2">
          {(["stdio", "http", "sse"] as const).map((t) => (
            <button
              key={t}
              onClick={() => setTransport(t)}
              className={`rounded-lg border px-3 py-1.5 text-xs ${
                transport === t
                  ? "border-accent bg-accent/5 text-accent"
                  : "border-line hover:bg-panel-2"
              }`}
            >
              {t === "stdio" ? "Local command" : t.toUpperCase()}
            </button>
          ))}
        </div>

        {transport === "stdio" ? (
          <>
            <label className="mt-4 block text-xs font-medium text-ink-dim">Command</label>
            <input
              value={command}
              onChange={(e) => setCommand(e.target.value)}
              placeholder="npx -y @playwright/mcp"
              className="mt-1 w-full rounded-lg border border-line bg-panel px-3 py-2 font-mono text-sm outline-none focus:border-accent"
            />
            <label className="mt-4 block text-xs font-medium text-ink-dim">
              Environment (one KEY=value per line)
            </label>
            <textarea
              value={env}
              onChange={(e) => setEnv(e.target.value)}
              rows={3}
              placeholder="DATABASE_URL=postgres://localhost/app"
              className="mt-1 w-full resize-none rounded-lg border border-line bg-panel px-3 py-2 font-mono text-xs outline-none focus:border-accent"
            />
            <div className="mt-1 text-[11px] text-ink-dim">
              Anthropic API keys are refused here — aichip runs on your CLI's own
              login and never handles credentials.
            </div>
          </>
        ) : (
          <>
            <label className="mt-4 block text-xs font-medium text-ink-dim">URL</label>
            <input
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              placeholder="https://example.com/mcp"
              className="mt-1 w-full rounded-lg border border-line bg-panel px-3 py-2 font-mono text-sm outline-none focus:border-accent"
            />
          </>
        )}

        {error && (
          <div className="mt-3 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">{error}</div>
        )}

        <div className="mt-5 flex justify-end gap-2">
          <button onClick={onClose} className="rounded-lg px-4 py-2 text-sm text-ink-dim hover:text-ink">
            Cancel
          </button>
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={save}
            disabled={busy || !name.trim()}
            className="rounded-lg bg-accent px-5 py-2 text-sm font-medium text-white disabled:opacity-50"
          >
            {busy ? "Saving…" : "Save"}
          </motion.button>
        </div>
      </motion.div>
    </motion.div>
  );
}

/** `KEY=value` lines to an object. Values may contain `=`; keys may not. */
function parseEnv(text: string): Record<string, string> {
  const out: Record<string, string> = {};
  for (const line of text.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const eq = trimmed.indexOf("=");
    if (eq <= 0) continue;
    out[trimmed.slice(0, eq).trim()] = trimmed.slice(eq + 1).trim();
  }
  return out;
}
