import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { motion } from "framer-motion";
import { api, Research, ResearchDetail as Detail, Project } from "../lib/api";
import { useWorkspace } from "../lib/workspace";
import { useEngines } from "../lib/engines";
import { NARROW, useMediaQuery } from "../lib/useMediaQuery";
import { useRunStream } from "../lib/ws";
import { RunStream } from "../components/RunStream";
import { Markdown } from "../components/Markdown";
import { isActive } from "../lib/runStatus";

/**
 * Deep research: ask a question about a project, watch the investigation,
 * read the cited report — and file it into the knowledge base when it is
 * worth keeping.
 *
 * The rail lists past researches for the picked project; the main pane is
 * either the composer (nothing selected), the live run, or the report. One
 * page for both routes — `/research` and `/research/:researchId` — so a
 * report is linkable.
 */
const PROJECT_KEY = "aichip.research.project";
/** The picker value for a research attached to no project: web-only. */
const GENERAL = "general";

export default function ResearchPage() {
  const { active } = useWorkspace();
  const engines = useEngines() ?? [];
  const narrow = useMediaQuery(NARROW);
  const navigate = useNavigate();
  const { researchId } = useParams();
  const [params, setParams] = useSearchParams();

  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState<string | null>(null);
  const [list, setList] = useState<Research[]>([]);
  const [question, setQuestion] = useState("");
  const [engine, setEngine] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [railOpen, setRailOpen] = useState(false);

  const workspaceId = active?.id ?? null;
  useEffect(() => {
    if (!workspaceId) return;
    api
      .projects(workspaceId)
      .then((r) => {
        setProjects(r.projects);
        const fromUrl = params.get("project");
        const remembered = localStorage.getItem(PROJECT_KEY);
        // General is the default: a question does not need a repository, and
        // a general research answers it from the web alone.
        const pick =
          (fromUrl === GENERAL ? GENERAL : r.projects.find((p) => p.id === fromUrl)?.id) ??
          (remembered === GENERAL ? GENERAL : r.projects.find((p) => p.id === remembered)?.id) ??
          GENERAL;
        setProjectId(pick);
      })
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspaceId]);

  const general = projectId === GENERAL;
  const refreshList = useCallback(() => {
    if (!projectId) return Promise.resolve();
    const scope =
      projectId === GENERAL
        ? workspaceId
          ? { workspaceId }
          : null
        : { projectId };
    if (!scope) return Promise.resolve();
    return api
      .researchList(scope)
      .then((r) => setList(r.researches))
      .catch(() => {});
  }, [projectId, workspaceId]);

  useEffect(() => {
    refreshList();
  }, [refreshList]);

  const pickProject = (id: string) => {
    setProjectId(id);
    localStorage.setItem(PROJECT_KEY, id);
    setParams({ project: id }, { replace: true });
    navigate(`/research?project=${id}`);
  };

  const start = async () => {
    if (!projectId || !question.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      const scope = general ? { workspaceId: workspaceId! } : { projectId };
      const r = await api.researchCreate(scope, question.trim(), engine ?? undefined);
      setQuestion("");
      refreshList();
      navigate(`/research/${r.id}`);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const rail = (
    <div className="flex min-h-0 flex-col gap-3 p-3">
      <select
        value={projectId ?? GENERAL}
        onChange={(e) => pickProject(e.target.value)}
        className="w-full rounded-lg border border-line bg-panel px-2 py-1.5 text-sm"
        title="General researches the web alone; pick a project to ground the answer in its repository"
      >
        <option value={GENERAL}>General — web only</option>
        {projects.map((p) => (
          <option key={p.id} value={p.id}>
            {p.name}
          </option>
        ))}
      </select>
      <Link
        to={projectId ? `/research?project=${projectId}` : "/research"}
        onClick={() => setRailOpen(false)}
        className="rounded-lg border border-line px-2 py-1.5 text-center text-sm text-ink-dim hover:bg-panel-2 hover:text-ink"
      >
        + New research
      </Link>
      <div className="min-h-0 flex-1 overflow-y-auto">
        {list.length === 0 && (
          <div className="px-2 py-2 text-xs text-ink-dim">Nothing researched yet.</div>
        )}
        {list.map((r) => (
          <Link
            key={r.id}
            to={`/research/${r.id}`}
            onClick={() => setRailOpen(false)}
            className={`block rounded-lg px-2 py-1.5 text-sm ${
              r.id === researchId ? "bg-panel-2 font-medium" : "hover:bg-panel-2"
            }`}
          >
            <span className="flex items-center gap-1.5">
              <StatusDot status={r.runStatus} hasReport={r.hasReport} />
              <span className="min-w-0 flex-1 truncate">{r.title || r.question}</span>
            </span>
          </Link>
        ))}
      </div>
    </div>
  );

  const main = researchId ? (
    <ResearchView
      id={researchId}
      onChanged={refreshList}
      // A deep link opens a research whose project the rail has never heard
      // of — sync the rail to it, or the list beside the report shows some
      // other project's work.
      onProject={(pid) => {
        const target = pid ?? GENERAL;
        if (target !== projectId) {
          setProjectId(target);
          localStorage.setItem(PROJECT_KEY, target);
        }
      }}
    />
  ) : (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <div className="mx-auto w-full max-w-3xl px-5 py-10">
        <h1 className="text-[26px] font-bold leading-tight tracking-tight">Deep research</h1>
        <p className="mt-1.5 text-sm leading-relaxed text-ink-dim">
          {general
            ? "Ask anything. The agent searches the web, reads the sources, and writes a report that cites every claim."
            : "Ask a question about this project. The agent reads the repository first, then the web, and writes a report that cites both — every web claim with its URL, every repo claim with its file."}
        </p>
        {error && (
          <div className="mt-4 rounded-lg bg-red-50 px-3 py-1.5 text-xs text-danger">{error}</div>
        )}
        <div className="mt-6 flex flex-col gap-2 rounded-xl border border-line bg-panel p-3 focus-within:border-accent">
          <textarea
            value={question}
            onChange={(e) => setQuestion(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey) {
                e.preventDefault();
                start();
              }
            }}
            rows={3}
            placeholder="e.g. What test framework does this repo use, and is it the current recommended one?"
            className="min-w-0 resize-none bg-transparent text-sm outline-none"
          />
          <div className="flex items-center justify-between">
            {engines.length > 1 ? (
              <select
                value={engine ?? ""}
                onChange={(e) => setEngine(e.target.value || null)}
                className="rounded-lg border border-line bg-panel px-2 py-1 text-xs text-ink-dim"
              >
                <option value="">Default engine</option>
                {engines.map((eng) => (
                  <option key={eng.id} value={eng.id}>
                    {eng.label || eng.id}
                  </option>
                ))}
              </select>
            ) : (
              <span />
            )}
            <motion.button
              whileTap={{ scale: 0.95 }}
              onClick={start}
              disabled={busy || !question.trim()}
              className="rounded-lg bg-accent px-4 py-1.5 text-sm text-white disabled:opacity-40"
            >
              {busy ? "Starting…" : "Research"}
            </motion.button>
          </div>
        </div>
      </div>
    </div>
  );

  if (narrow) {
    return (
      <div className="flex h-full min-h-0 flex-col">
        <button
          onClick={() => setRailOpen((o) => !o)}
          className="border-b border-line px-4 py-2 text-left text-sm font-medium"
        >
          Research <span className="text-[10px] text-ink-dim">{railOpen ? "▴" : "▾"}</span>
        </button>
        {railOpen && <div className="max-h-64 overflow-y-auto border-b border-line">{rail}</div>}
        {main}
      </div>
    );
  }

  return (
    <div className="grid h-full min-h-0 grid-cols-[280px_minmax(0,1fr)]">
      <div className="min-h-0 overflow-hidden border-r border-line bg-panel">{rail}</div>
      {main}
    </div>
  );
}

function StatusDot({ status, hasReport }: { status: string | null; hasReport: boolean }) {
  const color = isActive(status)
    ? "bg-tier-medium animate-pulse"
    : hasReport
      ? "bg-tier-easy"
      : "bg-danger";
  return <span className={`h-1.5 w-1.5 shrink-0 rounded-full ${color}`} />;
}

/** One research: the live run while it works, the report when it is done. */
function ResearchView({
  id,
  onChanged,
  onProject,
}: {
  id: string;
  onChanged: () => void;
  onProject: (projectId: string | null) => void;
}) {
  const navigate = useNavigate();
  const [detail, setDetail] = useState<Detail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const live = detail != null && isActive(detail.runStatus);
  const events = useRunStream(live ? detail.runId : null);

  const load = useCallback(() => {
    api
      .researchGet(id)
      .then((d) => {
        setDetail(d);
        onProject(d.projectId);
      })
      .catch((e) => setError(String(e)));
    // onProject is a fresh closure every render; the id is what matters.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  useEffect(() => {
    setDetail(null);
    setError(null);
    load();
  }, [load]);

  // While the run is live, poll for the finished report; the stream shows
  // progress but the report row is what carries the result.
  useEffect(() => {
    if (!live) return;
    const t = setInterval(load, 3000);
    return () => clearInterval(t);
  }, [live, load]);

  const saveToKb = async () => {
    setSaving(true);
    setError(null);
    try {
      await api.researchSaveToKb(id);
      load();
      onChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const rerun = async () => {
    setError(null);
    try {
      await api.researchRerun(id);
      load();
      onChanged();
    } catch (e) {
      setError(String(e));
    }
  };

  const cancel = async () => {
    try {
      await api.researchCancel(id);
      load();
      onChanged();
    } catch (e) {
      setError(String(e));
    }
  };

  const remove = async () => {
    try {
      await api.researchDelete(id);
      onChanged();
      navigate("/research");
    } catch (e) {
      setError(String(e));
    }
  };

  if (!detail) {
    return (
      <div className="flex min-h-0 flex-1 items-center justify-center text-sm text-ink-dim">
        {error ?? "Loading…"}
      </div>
    );
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
      <div className="mx-auto w-full max-w-3xl px-5 py-8">
        <div className="text-xs text-ink-dim">Research</div>
        <h1 className="mt-0.5 text-xl font-bold leading-tight tracking-tight">
          {detail.title || detail.question}
        </h1>
        {detail.title && (
          <p className="mt-1 text-sm text-ink-dim">“{detail.question}”</p>
        )}

        <div className="mt-3 flex flex-wrap items-center gap-2">
          {live ? (
            <button
              onClick={cancel}
              className="rounded-lg border border-line px-3 py-1.5 text-xs hover:border-danger hover:text-danger"
            >
              Cancel
            </button>
          ) : (
            <>
              {detail.reportMd &&
                (detail.kbArticleId ? (
                  <Link
                    to={`/knowledge/${detail.kbArticleId}`}
                    className="rounded-lg bg-tier-easy px-3 py-1.5 text-xs font-medium text-surface"
                  >
                    Open in knowledge base →
                  </Link>
                ) : (
                  <button
                    onClick={saveToKb}
                    disabled={saving}
                    className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white disabled:opacity-50"
                  >
                    {saving ? "Saving…" : "Save to knowledge base"}
                  </button>
                ))}
              <button
                onClick={rerun}
                className="rounded-lg border border-line px-3 py-1.5 text-xs hover:border-ink-dim"
                title="Ask again — the report is replaced by the new answer"
              >
                ↻ Re-run
              </button>
              <button
                onClick={remove}
                className="ml-auto rounded-lg border border-line px-3 py-1.5 text-xs text-ink-dim hover:border-danger hover:text-danger"
              >
                Delete
              </button>
            </>
          )}
        </div>

        {error && (
          <div className="mt-3 rounded-lg bg-red-50 px-3 py-1.5 text-xs text-danger">{error}</div>
        )}
        {!live && !detail.reportMd && detail.runError && (
          <div className="mt-3 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">
            The run stopped: {detail.runError}
          </div>
        )}

        {live && (
          <div className="mt-5 rounded-xl border border-line bg-panel p-3">
            <div className="mb-2 text-xs font-medium text-ink-dim">Investigating…</div>
            <RunStream events={events} empty="Waiting for the agent to start…" />
          </div>
        )}

        {!live && detail.reportMd && (
          <div className="md mt-6">
            <Markdown>{detail.reportMd}</Markdown>
          </div>
        )}
      </div>
    </div>
  );
}
