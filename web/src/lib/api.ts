import { TreePage } from "./kbTree";
export type Tier = "easy" | "medium" | "complex";

export interface Workspace {
  id: string;
  name: string;
  icon: string;
  color: string;
}

export interface Project {
  id: string;
  path: string;
  name: string;
  defaultBranch: string;
  workspaceId: string;
  /** "git" — isolated worktrees, diffs, review. "none" — tasks edit in place. */
  vcs: "git" | "none";
  /** Why a project has no version control. Null for git projects. */
  vcsNote: string | null;
  /** Agents may work here without stopping to ask. Requires a git project. */
  fullAutoOptIn: boolean;
}

export type PermissionMode = "reviewed" | "auto_edit" | "full_auto";

export interface PermissionSettings {
  defaultMode: PermissionMode;
  /** Agents pinning their own mode — each one ignores `defaultMode`. */
  agentsOverriding: number;
  modes: { id: PermissionMode; label: string; blurb: string }[];
}

export interface Task {
  id: string;
  title: string;
  modelTier: Tier;
  boardColumn: "backlog" | "running" | "review" | "done";
  /** Manual kanban ordering within a column; smaller sorts first. */
  position: number;
  branch: string | null;
  projectId: string;
  agentId: string | null;
  agentName: string | null;
  agentColor: string | null;
  teamId: string | null;
  teamName: string | null;
  teamPattern: string | null;
  /** The epic this is a sub-ticket of, if any. */
  parentId: string | null;
  parentTitle: string | null;
  /**
   * Sub-tickets under this card. `childCount > 0` is what makes a card an epic
   * — it is derived rather than stored, so it cannot disagree with the board.
   */
  childCount: number;
  childResolved: number;
  /**
   * The raw status of the assignment this card came from, when it came from one.
   * The four board columns have nowhere to put "failed" or "dropped", so the
   * column carries position and this carries the outcome.
   */
  stepStatus: string | null;
  /**
   * The mode this card will actually run under, and which of the three places
   * decided it — the bound agent's preset, the card's own, or the machine
   * default, in that order of precedence.
   *
   * Surfaced because the order surprises people: a project set to work without
   * asking still stops for permission when its agent carries its own preset,
   * and until this there was nothing anywhere that said so.
   */
  effectiveMode: "reviewed" | "auto_edit" | "full_auto";
  permissionSource: "agent" | "card" | "default";
  /** Set when the latest run was an organization run — opens the team room. */
  orgRunId: string | null;
  runId: string | null;
  runStatus: string | null;
  costUsd: number | null;
  model: string | null;
  /** Which CLI this card runs on. */
  engine: string;
  /** Draft a plan and wait for approval before doing any work. */
  planFirst: boolean;
  /** This card's own thinking budget. Null means inherit. */
  effort: Effort | null;
  /**
   * What the card will actually think with, and which of the four places
   * decided it — the bound agent's budget, the card's own, its tier on the
   * engine it runs on, or the machine default. Surfaced for the same reason
   * `permissionSource` is: the order is not guessable from any one screen.
   */
  effectiveEffort: Effort | null;
  effortSource: "agent" | "card" | "tier" | "default";
}

/** A knowledge-base page. `contentHtml` is absent in list responses. */
export interface Article {
  id: string;
  workspaceId: string;
  title: string;
  summary: string;
  status: "draft" | "published";
  origin: "human" | "agent";
  sourceRunId: string | null;
  updatedAt: string;
  parentId: string | null;
  projectId: string | null;
  icon: string;
  position: number;
  /** The newest accepted revision. A label, not a lock. */
  currentSeq: number;
  /**
   * The token a save must match. Not `currentSeq`, which holds still while
   * rapid autosaves coalesce into one revision — so two editors both matched it
   * and the second silently overwrote the first.
   */
  bodyVersion: number;
  contentHtml?: string;
  // Present only on the single-page read.
  breadcrumb?: { id: string; title: string; icon: string }[];
  children?: { id: string; title: string; icon: string; summary: string }[];
  backlinks?: { id: string; title: string; icon: string }[];
  usedBy?: UsedBy;
  pendingRevision?: Revision | null;
  writing?: boolean;
}

/**
 * The board cards that depend on this page.
 *
 * `total` is the true count; `tasks` is capped server-side, so a page every card
 * references says how many it is not showing instead of pretending the list is
 * the whole story.
 */
export interface UsedBy {
  total: number;
  tasks: {
    id: string;
    title: string;
    projectId: string;
    projectName: string;
    boardColumn: string;
    /** Attached to the card, so every run on it is handed this page. */
    attached: boolean;
    /** How many comments linked it — a mention reaches one reply, not the card. */
    mentions: number;
  }[];
}


/**
 * A card's branch, built and running so you can look at it.
 *
 * `url` is present only while `status === "running"` — a link to a container
 * that is still building is a link to a connection refused.
 */
export interface TaskPreview {
  id: string;
  /** Null for a project's base-branch preview, which belongs to no card. */
  taskId: string | null;
  status: "building" | "running" | "stopped" | "failed";
  url: string | null;
  hostPort: number | null;
  containerPort: number | null;
  /** The Dockerfile named no port, so the one above is a guess. */
  portAssumed: boolean;
  /** The card was worked on after this was built, so it is serving history. */
  stale: boolean;
  /** Its image is still on disk, so starting again is a wake, not a rebuild. */
  canWake: boolean;
  /**
   * The hostname label this answers to, while it is running.
   *
   * Prefer this over `url`: a port is asked of the OS on every start, so the
   * port URL changes under a bookmark, and every preview on 127.0.0.1 shares
   * one cookie jar. `previewUrl()` builds the address.
   */
  slug: string | null;
  error: string | null;
}

/**
 * Where to send someone for a preview.
 *
 * The port is this page's own — aichip proxies preview hostnames on the port it
 * is already served on, so the browser's location is the authority on it and
 * the server never has to be told which port it is behind.
 */
export function previewUrl(p: TaskPreview): string | null {
  if (p.slug) {
    const port = window.location.port ? `:${window.location.port}` : "";
    return `http://${p.slug}.preview.localhost${port}`;
  }
  return p.url;
}

/**
 * A build recipe for a project with no Dockerfile of its own.
 *
 * `proposed` means an agent wrote it and nobody has read it. Nothing builds a
 * proposal — a Dockerfile's RUN lines execute on this machine, so approving one
 * is approving code, and the UI shows the whole text rather than a summary.
 */
export interface PreviewRecipe {
  dockerfile: string;
  status: "proposed" | "approved";
  /** A person rewrote it rather than approving what was proposed. */
  edited: boolean;
}

/** One row in a project's Previews tab. */
export interface ProjectPreview {
  id: string;
  /** Null for the base branch, which belongs to no card. */
  taskId: string | null;
  /** The card's title, or "main". */
  title: string;
  status: "building" | "running" | "idle" | "failed";
  url: string | null;
  hostPort: number | null;
  containerPort: number | null;
  portAssumed: boolean;
  stale: boolean;
  canWake: boolean;
  slug: string | null;
  error: string | null;
}

/** Whether previews are possible here at all. */
export interface DockerStatus {
  installed: boolean;
  usable: boolean;
  version?: string;
  /** Why not, phrased for someone who now has to go and fix it. */
  problem?: string;
}

/** Whether this machine can talk to GitHub, and as whom. No token ever crosses this. */
export interface GitHubStatus {
  installed: boolean;
  usable: boolean;
  version?: string;
  accounts: {
    host: string;
    login: string;
    active: boolean;
    valid: boolean;
    /** gh's own words for why this login can't be used. */
    problem: string | null;
  }[];
}

/** One entry in a page's history. */
export interface Revision {
  seq: number;
  kind: "edit" | "agent" | "restore" | "import";
  state: "pending" | "accepted" | "discarded" | "superseded";
  authorKind: "human" | "agent";
  title: string;
  baseSeq: number | null;
  restoredFrom: number | null;
  runId: string | null;
  note: string;
  createdAt: string;
  chars: number;
}

export interface RevisionDiff {
  from: number | null;
  to: number;
  added: number;
  removed: number;
  diff: string;
}

/** A space is a repository; `id: null` is the workspace-wide General space. */
export interface Space {
  id: string | null;
  name: string;
  pages: number;
}

/** Raised when a page moved on while you were editing it. */
export class ConflictError extends Error {}

/** An article as it appears when tagged onto a card. */
export interface LinkedArticle {
  id: string;
  title: string;
  summary: string;
  status: string;
  origin: string;
}

/** The plan a parked run is waiting on. */
export interface TaskPlan {
  runId: string;
  content: string | null;
  /** Only while true can it be approved, edited, or sent back. */
  awaitingApproval: boolean;
  /** A person rewrote it, rather than approving what was proposed. */
  edited: boolean;
  writtenAt: string | null;
}

export interface Agent {
  id: string;
  name: string;
  icon: string;
  color: string;
  description: string;
  systemPrompt: string;
  modelTier: Tier;
  allowedTools: string[];
  /** null = inherit the workspace default. */
  permissionPreset: string | null;
  /** null = leave the CLI's own default alone. */
  effort: Effort | null;
  /** null = inherit whatever the card says. */
  engine: string | null;
  builtin: boolean;
}

export interface AgentDraft {
  name: string;
  icon?: string;
  color?: string;
  description?: string;
  system_prompt?: string;
  model_tier?: Tier;
  permission_preset?: string;
  allowed_tools?: string[];
}

export interface Team {
  id: string;
  name: string;
  /** null = inherit from the card that summoned it. */
  engine: string | null;
  pattern: "pipeline" | "debate" | "swarm" | "org";
  definition: {
    manager?: string;
    members?: { agent_id: string; role?: string }[];
  };
}

export interface OrgMember {
  name: string;
  title: string;
  color: string;
  description: string;
  isManager: boolean;
}

export type Effort = "low" | "medium" | "high" | "xhigh" | "max";

export interface OrgAssignment {
  id: string;
  key: string;
  status: string;
  assignee: string | null;
  title: string | null;
  brief: string | null;
  output: string | null;
  dependsOn: string[];
  doneWhen: string[];
  /** Files this assignment declared it would change; drives parallelism. */
  touches: string[];
  size: string | null;
  origin: string;
  attempt: number;
  /** "manager" steps are planning, not work handed to a specialist. */
  kind: "manager" | "assignment";
  startedAt: string | null;
  finishedAt: string | null;
}

export interface OrgMessage {
  id: string;
  seq: number;
  from: string;
  to: string | null;
  kind: "assignment" | "message" | "question" | "answer" | "status" | "result";
  content: string;
  ts: string;
}

export interface OrgRunDetail {
  id: string;
  teamId: string;
  teamName: string;
  goal: string | null;
  status: string;
  costUsd: number | null;
  error: string | null;
  roster: OrgMember[];
  assignments: OrgAssignment[];
  messages: OrgMessage[];
}

export interface OrgRunSummary {
  id: string;
  teamId: string;
  teamName: string;
  goal: string | null;
  status: string;
  costUsd: number | null;
  error: string | null;
  createdAt: string;
}

export interface ChatMessage {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
  runId: string | null;
  ts: string;
  attachments: Attachment[];
}

export interface Attachment {
  id: string;
  filename: string;
  mime: string;
  kind: "image" | "pdf" | "text";
  size: number;
}

/** Extensions the server accepts; mirrored here only to fail fast in the UI. */
export const ATTACHMENT_ACCEPT =
  ".png,.jpg,.jpeg,.gif,.webp,.pdf,.txt,.md,.csv,.json,.log";
export const MAX_ATTACHMENT_BYTES = 10 * 1024 * 1024;
export const MAX_ATTACHMENTS = 10;

export interface TaskComment {
  id: string;
  author: "user" | "agent";
  agentId: string | null;
  agentName: string | null;
  agentColor: string | null;
  content: string;
  runId: string | null;
  /** Set when the note was written against a line of the diff. */
  filePath: string | null;
  line: number | null;
  hunk: string | null;
  ts: string;
}

export interface AgentMemory {
  id: string;
  kind: string;
  content: string;
  projectName: string | null;
  ts: string;
}

export interface ChatSummary {
  /** Null means inherit — the machine default, resolved when the turn runs. */
  modelTier: Tier | null;
  effort: Effort | null;
  id: string;
  title: string;
  messageCount: number;
  updatedAt: string;
}

export interface FileEntry {
  name: string;
  /** Project-relative, forward-slashed. */
  path: string;
  kind: "dir" | "file";
  size: number | null;
}

export interface FileListing {
  path: string;
  /** null at the project root. */
  parent: string | null;
  entries: FileEntry[];
}

export interface FileContent {
  path: string;
  size: number;
  tooLarge: boolean;
  binary: boolean;
  /** null when the file is binary or too large to send. */
  content: string | null;
}

export interface SearchHit {
  id: string;
  label: string;
  sublabel: string;
  /** Set on tasks and workflows, so the client knows where to navigate. */
  projectId?: string;
}

export interface SearchResults {
  projects: SearchHit[];
  tasks: SearchHit[];
  agents: SearchHit[];
  teams: SearchHit[];
  workflows: SearchHit[];
}

export interface WorkflowDef {
  id: string;
  projectId: string;
  name: string;
  description: string;
  sourceYaml: string;
  cronExpr: string | null;
  enabled: boolean;
  catchUp: string;
  lastRunAt: string | null;
  nextRunAt: string | null;
  stepCount: number;
  error: string | null;
  uiLayout: Record<string, { x: number; y: number }>;
}

export interface WorkflowRun {
  id: string;
  workflowId: string;
  workflowName: string;
  status: string;
  trigger: string;
  costUsd: number | null;
  error: string | null;
  createdAt: string;
}

export interface RunStep {
  id: string;
  stepKey: string;
  status: string;
  output: string | null;
  startedAt: string | null;
  finishedAt: string | null;
}

export interface PendingPermission {
  requestId: string;
  toolName: string;
  input: unknown;
}

export interface ActivityRun {
  id: string;
  label: string;
  status: string;
  trigger: string;
  teamName: string | null;
  projectName: string | null;
  projectId: string | null;
  taskId: string | null;
  isOrg: boolean;
  costUsd: number | null;
  /** Which CLI is doing the work, and the exact model it resolved to. */
  engine: string;
  model: string | null;
  startedAt: string | null;
  createdAt: string;
}

/** Something that will not move until a person does something about it. */
export interface Blocker {
  runId: string;
  kind: "plan" | "permission";
  label: string;
  /** Plan blockers only: a team's plan opens the room, a card's opens the board. */
  isOrg?: boolean;
  projectId?: string | null;
  requestId?: string;
  tool?: string;
  input?: unknown;
}

/** Why the queue is or isn't dispatching. `over_budget` has no resume — it
 *  clears at midnight — so it must not be rendered as a pause. */
export type QueueGate =
  | { state: "open" }
  | { state: "paused" }
  | { state: "over_budget"; spentToday: number; capUsd: number };

export interface TeamEstimate {
  runs: number;
  medianUsd: number | null;
  worstUsd: number | null;
  medianSecs: number | null;
}

export interface Activity {
  paused: boolean;
  gate: QueueGate;
  /** Daily cap in dollars, or null when uncapped. */
  budgetUsd: number | null;
  live: ActivityRun[];
  blocked: Blocker[];
  spend: {
    today: number;
    window: number;
    daily: { day: string; cost: number; runs: number }[];
    byAgent: { name: string; cost: number; steps: number }[];
  };
}

/** Which model each complexity tier routes to, plus what may be chosen. */
/** One engine's tier routing, as the settings page edits it. */
export interface EngineModels {
  id: string;
  label: string;
  /** False for engines fronting many providers — the field is free text. */
  fixedCatalog: boolean;
  choices: { id: string; label: string; blurb: string }[];
  /** Model ids this install can actually reach. Suggestions, not a whitelist. */
  available: string[];
  providers: { name: string; auth: string }[];
  tiers: Record<Tier, string>;
  defaults: Record<Tier, string>;
  /**
   * How hard each tier thinks here. Null means the tier pins nothing.
   *
   * Optional because the dashboard is served from disk while the binary that
   * answers these calls is whatever is still running — a server started before
   * this field existed serves the new page and then omits it. Typed honestly
   * so the compiler makes every reader handle that.
   */
  efforts?: Record<Tier, Effort | null>;
}

/** The machine-wide thinking budget, and who ignores it. */
export interface EffortSettings {
  /** Null means every run keeps its CLI's own default — the shipped choice. */
  defaultEffort: Effort | null;
  /** Agents pinning their own budget, which outranks this. */
  agentsOverriding: number;
  levels: { id: Effort; label: string; blurb: string }[];
}

export interface ModelSettings {
  engines: EngineModels[];
}

/** One attempt in a bake-off, with the diff that decides it. */
export interface BakeoffVariant {
  runId: string;
  label: string;
  status: string;
  agentName: string | null;
  model: string | null;
  costUsd: number | null;
  error: string | null;
  seconds: number | null;
  linesChanged: number;
  diff: string;
}

/** An MCP server the user connected, giving agents tools beyond files+bash. */
export interface McpServer {
  id: string;
  name: string;
  transport: "stdio" | "http" | "sse";
  command: string | null;
  args: string[];
  env: Record<string, string>;
  url: string | null;
  headers: Record<string, string>;
  enabled: boolean;
  /** What the model sees its tools called, e.g. `mcp__playwright`. */
  toolPrefix: string;
}

export type McpTestResult =
  | { ok: true; tools: string[]; toolPrefix: string }
  | { ok: false; error: string };

export interface FsListing {
  path: string;
  parent: string | null;
  isGitRepo: boolean;
  dirs: { name: string; path: string; isGitRepo: boolean }[];
}

async function json<T>(res: Response): Promise<T> {
  if (!res.ok) throw new Error(await res.text());
  return res.json() as Promise<T>;
}

const post = (url: string, body?: unknown) =>
  fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });

const patch = (url: string, body: unknown) =>
  fetch(url, {
    method: "PATCH",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });

// Deliberately no headers: the browser must set multipart/form-data itself so
// it can include the boundary. Setting Content-Type here would omit it and the
// server would fail to parse the body.
const postForm = (url: string, form: FormData) =>
  fetch(url, { method: "POST", body: form });

export const api = {
  // Probed live rather than cached: `gh auth login` happens in a terminal
  // while aichip is running, and this is what tells you to go and do it.
  github: () => fetch("/api/github").then((r) => json<GitHubStatus>(r)),

  // workspaces
  workspaces: () =>
    fetch("/api/workspaces").then((r) => json<{ workspaces: Workspace[] }>(r)),
  createWorkspace: (name: string) =>
    post("/api/workspaces", { name }).then((r) => json<{ id: string }>(r)),
  renameWorkspace: (id: string, name: string) =>
    patch(`/api/workspaces/${id}`, { name }).then(json),

  // projects
  projects: (workspaceId: string) =>
    fetch(`/api/projects?workspace_id=${workspaceId}`).then((r) =>
      json<{ projects: Project[] }>(r),
    ),
  // Initializes a repository server-side when the folder needs one.
  addProject: (workspaceId: string, path: string) =>
    post("/api/projects", { workspace_id: workspaceId, path }).then((r) =>
      json<{ id: string; name: string; vcs: "git" | "none"; vcsNote: string | null }>(r),
    ),

  // fs browser
  fsList: (path?: string) =>
    fetch(`/api/fs/list${path ? `?path=${encodeURIComponent(path)}` : ""}`).then(
      (r) => json<FsListing>(r),
    ),
  gitInit: (path: string) => post("/api/fs/git-init", { path }).then(json),
  /** Create `name` inside `parent`, so a project can start from nothing. */
  fsMkdir: (parent: string, name: string) =>
    post("/api/fs/mkdir", { parent, name }).then((r) =>
      json<{ path: string; name: string; isGitRepo: boolean }>(r),
    ),

  // settings
  modelSettings: () => fetch("/api/settings/models").then((r) => json<ModelSettings>(r)),
  permissionSettings: () =>
    fetch("/api/settings/permissions").then((r) => json<PermissionSettings>(r)),
  setDefaultPermissionMode: (mode: PermissionMode) =>
    fetch("/api/settings/permissions", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ default_mode: mode }),
    }).then((r) => json<{ defaultMode: PermissionMode }>(r)),
  /** Clear every agent's own preset so they follow the workspace default. */
  applyPermissionsToAgents: () =>
    post("/api/settings/permissions/apply-to-agents").then((r) =>
      json<{ cleared: number }>(r),
    ),
  /** Let agents work in this project without stopping to ask. */
  setProjectFullAuto: (projectId: string, on: boolean) =>
    patch(`/api/projects/${projectId}`, { full_auto_opt_in: on }).then((r) =>
      json<Project>(r),
    ),
  effortSettings: () =>
    fetch("/api/settings/effort").then((r) => json<EffortSettings>(r)),
  setDefaultEffort: (effort: Effort | null) =>
    fetch("/api/settings/effort", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ default_effort: effort }),
    }).then((r) => json<{ defaultEffort: Effort | null }>(r)),
  setModelSettings: (
    engines: Record<string, Record<Tier, string>>,
    efforts: Record<string, Record<Tier, Effort | null>>,
  ) =>
    fetch("/api/settings/models", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ engines, efforts }),
    }).then((r) => json<{ saved: boolean }>(r)),

  // MCP servers the user connects
  mcpServers: (workspaceId: string) =>
    fetch(`/api/mcp-servers?workspace_id=${workspaceId}`).then((r) =>
      json<{ servers: McpServer[] }>(r),
    ),
  createMcpServer: (body: Record<string, unknown>) =>
    post("/api/mcp-servers", body).then((r) => json<McpServer>(r)),
  updateMcpServer: (id: string, body: Record<string, unknown>) =>
    patch(`/api/mcp-servers/${id}`, body).then((r) => json<McpServer>(r)),
  deleteMcpServer: (id: string) =>
    fetch(`/api/mcp-servers/${id}`, { method: "DELETE" }).then(json),
  /** Connect and ask what tools it offers. Slow by nature — it starts the server. */
  testMcpServer: (id: string) =>
    post(`/api/mcp-servers/${id}/test`).then((r) => json<McpTestResult>(r)),
  agentMcpServers: (agentId: string) =>
    fetch(`/api/agents/${agentId}/mcp-servers`).then((r) =>
      json<{ serverIds: string[] }>(r),
    ),
  setAgentMcpServers: (agentId: string, serverIds: string[]) =>
    fetch(`/api/agents/${agentId}/mcp-servers`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ server_ids: serverIds }),
    }).then((r) => json<{ serverIds: string[] }>(r)),

  // tasks
  tasks: (opts: { workspaceId?: string; projectId?: string }) => {
    const params = new URLSearchParams();
    if (opts.workspaceId) params.set("workspace_id", opts.workspaceId);
    if (opts.projectId) params.set("project_id", opts.projectId);
    return fetch(`/api/tasks?${params}`).then((r) => json<{ tasks: Task[] }>(r));
  },
  createTask: (body: {
    project_id: string;
    title: string;
    prompt: string;
    model_tier: Tier;
    start: boolean;
    agent_id?: string | null;
    team_id?: string | null;
    engine?: string;
    plan_first?: boolean;
    effort?: Effort | null;
    article_ids?: string[];
    attachment_ids?: string[];
  }) => post("/api/tasks", body).then((r) => json<{ id: string; runId: string | null }>(r)),
  startTask: (taskId: string) =>
    post(`/api/tasks/${taskId}/start`).then((r) => json<{ runId: string }>(r)),
  deleteTask: (taskId: string) =>
    fetch(`/api/tasks/${taskId}`, { method: "DELETE" }).then((r) =>
      json<{ deleted: boolean }>(r),
    ),
  /** `fresh` discards the previous attempt's worktree and its unmerged diff. */
  retryTask: (taskId: string, fresh = true) =>
    post(`/api/tasks/${taskId}/retry`, { fresh }).then((r) =>
      json<{ runId: string; fresh: boolean }>(r),
    ),
  // Kanban drag: moving into "running" from backlog starts the task.
  moveTask: (
    taskId: string,
    body: {
      board_column?: string;
      position?: number;
      engine?: string;
      plan_first?: boolean;
      model_tier?: Tier;
      /**
       * Three states, and JSON gives us all three: omit the key to leave it
       * alone, send null to return the card to inheriting, send a value to pin
       * one. Passing `undefined` omits it, which is what you want.
       */
      effort?: Effort | null;
    },
  ) =>
    patch(`/api/tasks/${taskId}`, body).then((r) =>
      json<{ moved: boolean; runId: string | null }>(r),
    ),
  projectPreviews: (projectId: string) =>
    fetch(`/api/projects/${projectId}/previews`).then((r) =>
      json<{
        previews: ProjectPreview[];
        live: number;
        maxLive: number;
        diskBytes: number;
        reclaimable: number;
      }>(r),
    ),
  /** The project's base branch, running — what a card's changes compare to. */
  basePreview: (projectId: string) =>
    fetch(`/api/projects/${projectId}/preview`).then((r) =>
      json<{ preview: TaskPreview | null }>(r),
    ),
  startBasePreview: (projectId: string) =>
    post(`/api/projects/${projectId}/preview`).then((r) =>
      json<{ preview: TaskPreview }>(r),
    ),
  stopBasePreview: (projectId: string) =>
    fetch(`/api/projects/${projectId}/preview`, { method: "DELETE" }).then((r) =>
      json<{ stopped: boolean }>(r),
    ),
  /** A Dockerfile an agent wrote for a project that has none. */
  previewRecipe: (projectId: string) =>
    fetch(`/api/projects/${projectId}/preview-recipe`).then((r) =>
      json<{ recipe: PreviewRecipe | null }>(r),
    ),
  proposeRecipe: (projectId: string) =>
    post(`/api/projects/${projectId}/preview-recipe`).then((r) =>
      json<{ recipe: PreviewRecipe }>(r),
    ),
  approveRecipe: (projectId: string, dockerfile: string) =>
    fetch(`/api/projects/${projectId}/preview-recipe`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ dockerfile }),
    }).then((r) => json<{ approved: boolean; edited: boolean }>(r)),
  // Previews
  previewLimits: () =>
    fetch("/api/previews/limits").then((r) =>
      json<{ maxLive: number; idleMinutes: number; live: number }>(r),
    ),
  setPreviewLimits: (maxLive: number, idleMinutes: number) =>
    fetch("/api/previews/limits", {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ max_live: maxLive, idle_minutes: idleMinutes }),
    }).then((r) => json<{ maxLive: number; idleMinutes: number }>(r)),
  previewDisk: () =>
    fetch("/api/previews/disk").then((r) =>
      json<{ bytes: number; reclaimable: number }>(r),
    ),
  reclaimPreviewDisk: () =>
    fetch("/api/previews/disk", { method: "DELETE" }).then((r) =>
      json<{ reclaimed: number }>(r),
    ),
  dockerStatus: () => fetch("/api/docker").then((r) => json<DockerStatus>(r)),
  taskPreview: (taskId: string) =>
    fetch(`/api/tasks/${taskId}/preview`).then((r) =>
      json<{ preview: TaskPreview | null }>(r),
    ),
  startPreview: (taskId: string) =>
    post(`/api/tasks/${taskId}/preview`).then((r) =>
      json<{ preview: TaskPreview }>(r),
    ),
  stopPreview: (taskId: string) =>
    fetch(`/api/tasks/${taskId}/preview`, { method: "DELETE" }).then((r) =>
      json<{ stopped: boolean }>(r),
    ),
  // Knowledge base
  articles: (workspaceId: string, q?: string) =>
    fetch(
      `/api/kb/articles?workspace_id=${workspaceId}` +
        (q ? `&q=${encodeURIComponent(q)}` : ""),
    ).then((r) => json<{ articles: Article[] }>(r)),
  article: (id: string) => fetch(`/api/kb/articles/${id}`).then((r) => json<Article>(r)),
  createArticle: (body: {
    workspace_id: string;
    title: string;
    content_html?: string;
    status?: string;
    parent_id?: string | null;
    project_id?: string | null;
    icon?: string;
    asset_ids?: string[];
  }) => post("/api/kb/articles", body).then((r) => json<Article>(r)),
  updateArticle: async (
    id: string,
    body: {
      title?: string;
      content_html?: string;
      status?: string;
      icon?: string;
      project_id?: string | null;
      /** What the new revision diffs against. */
      base_seq?: number;
      /** The concurrency guard. Required whenever content_html is sent. */
      base_version?: number;
      asset_ids?: string[];
    },
  ) => {
    const r = await patch(`/api/kb/articles/${id}`, body);
    // A stale editor is a decision for the user, not a failure to swallow.
    if (r.status === 409) throw new ConflictError(await r.text());
    return json<Article>(r);
  },

  // Structure
  kbSpaces: (workspaceId: string) =>
    fetch(`/api/kb/spaces?workspace_id=${workspaceId}`).then((r) =>
      json<{ spaces: Space[] }>(r),
    ),
  kbTree: (workspaceId: string, projectId: string | null) =>
    fetch(
      `/api/kb/tree?workspace_id=${workspaceId}` +
        (projectId ? `&project_id=${projectId}` : ""),
    ).then((r) => json<{ pages: TreePage[] }>(r)),
  movePage: (id: string, parentId: string | null, afterId?: string | null) =>
    post(`/api/kb/articles/${id}/move`, {
      parent_id: parentId,
      after_id: afterId ?? null,
    }).then((r) => json<{ moved: boolean }>(r)),

  // History
  revisions: (id: string) =>
    fetch(`/api/kb/articles/${id}/revisions`).then((r) =>
      json<{ revisions: Revision[] }>(r),
    ),
  revisionDiff: (id: string, to: number, from?: number) =>
    fetch(
      `/api/kb/articles/${id}/diff?to=${to}` + (from !== undefined ? `&from=${from}` : ""),
    ).then((r) => json<RevisionDiff>(r)),
  acceptRevision: (id: string, seq: number) =>
    post(`/api/kb/articles/${id}/revisions/${seq}/accept`).then((r) =>
      json<{ accepted: boolean; seq: number }>(r),
    ),
  discardRevision: (id: string, seq: number, note: string) =>
    post(`/api/kb/articles/${id}/revisions/${seq}/discard`, { note }).then((r) =>
      json<{ discarded: boolean }>(r),
    ),
  restoreRevision: (id: string, seq: number) =>
    post(`/api/kb/articles/${id}/restore`, { seq }).then((r) =>
      json<{ restored: boolean; seq: number }>(r),
    ),
  deleteArticle: (id: string) =>
    fetch(`/api/kb/articles/${id}`, { method: "DELETE" }).then((r) =>
      json<{ deleted: boolean }>(r),
    ),
  /** Ask an agent to write one. Returns the run that will fill it in. */
  generateArticle: (body: {
    workspace_id: string;
    project_id: string;
    brief: string;
    engine?: string;
    parent_id?: string;
  }) =>
    post("/api/kb/generate", body).then((r) =>
      json<{ runId: string; articleId: string | null }>(r),
    ),
  reviseArticle: (
    id: string,
    body: { project_id: string; brief: string; engine?: string },
  ) =>
    post(`/api/kb/articles/${id}/generate`, body).then((r) =>
      json<{ runId: string }>(r),
    ),

  /** Articles tagged onto a card — the agent reads them before it starts. */
  taskArticles: (taskId: string) =>
    fetch(`/api/tasks/${taskId}/articles`).then((r) =>
      json<{ articles: LinkedArticle[] }>(r),
    ),
  setTaskArticles: (taskId: string, articleIds: string[]) =>
    fetch(`/api/tasks/${taskId}/articles`, {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ article_ids: articleIds }),
    }).then((r) => json<{ linked: number }>(r)),

  // Plan-first cards
  taskPlan: (runId: string) =>
    fetch(`/api/runs/${runId}/plan`).then((r) => json<TaskPlan>(r)),
  saveTaskPlan: (runId: string, content: string) =>
    patch(`/api/runs/${runId}/plan`, { content }).then((r) =>
      json<{ saved: boolean }>(r),
    ),
  approveTaskPlan: (runId: string) =>
    post(`/api/runs/${runId}/plan/approve`).then((r) =>
      json<{ approved: boolean }>(r),
    ),
  reviseTaskPlan: (runId: string, note: string) =>
    post(`/api/runs/${runId}/plan/revise`, { note }).then((r) =>
      json<{ revising: boolean }>(r),
    ),

  taskComments: (taskId: string) =>
    fetch(`/api/tasks/${taskId}/comments`).then((r) =>
      json<{ comments: TaskComment[]; pendingReplies: number }>(r),
    ),
  /** `anchor` attaches the note to a line of the diff; `fix: true` turns it
   *  into a scoped run in the task's existing worktree. */
  postComment: (
    taskId: string,
    content: string,
    engine?: string,
    articleIds?: string[],
    anchor?: { file_path?: string; line?: number; hunk?: string; fix?: boolean },
  ) =>
    post(`/api/tasks/${taskId}/comments`, {
      content,
      engine,
      article_ids: articleIds,
      ...anchor,
    }).then((r) =>
      json<{ id: string; runIds: string[]; fixRunId?: string }>(r),
    ),
  attachToTask: (taskId: string, attachmentIds: string[]) =>
    post(`/api/tasks/${taskId}/attachments/claim`, { attachment_ids: attachmentIds }).then(
      (r) => json<{ attached: number }>(r),
    ),
  diff: (taskId: string) =>
    fetch(`/api/tasks/${taskId}/diff`).then((r) => json<{ diff: string }>(r)),
  merge: (taskId: string) =>
    post(`/api/tasks/${taskId}/merge`).then((r) => json<{ merged: boolean }>(r)),
  cancelRun: (runId: string) => post(`/api/runs/${runId}/cancel`),

  /** Hand a card to someone else, or to nobody.
   *
   *  `null` clears the assignment; omitting a field leaves it alone. The two
   *  are different requests, so both ids are always sent explicitly. */
  reassignTask: (taskId: string, assignee: { kind: "agent" | "team"; id: string } | null) =>
    patch(`/api/tasks/${taskId}`, {
      agent_id: assignee?.kind === "agent" ? assignee.id : null,
      team_id: assignee?.kind === "team" ? assignee.id : null,
    }).then(json),

  // bake-off: one brief, several attempts, keep the best
  startBakeoff: (
    taskId: string,
    variants: { label: string; agent_id?: string; tier?: string; engine?: string }[],
  ) =>
    post(`/api/tasks/${taskId}/bakeoff`, { variants }).then((r) =>
      json<{ runIds: string[] }>(r),
    ),
  bakeoff: (taskId: string) =>
    fetch(`/api/tasks/${taskId}/bakeoff`).then((r) =>
      json<{ variants: BakeoffVariant[] }>(r),
    ),
  /** Adopt this variant's worktree as the task's; the others are discarded. */
  keepVariant: (runId: string) =>
    post(`/api/runs/${runId}/keep`).then((r) => json<{ kept: string }>(r)),
  pendingPermissions: (runId: string) =>
    fetch(`/api/runs/${runId}/pending-permissions`).then((r) =>
      json<{ pending: PendingPermission[] }>(r),
    ),
  resolvePermission: (requestId: string, allowed: boolean) =>
    post(`/api/permissions/${requestId}/resolve`, { allowed }),

  // activity
  activity: (workspaceId?: string) =>
    fetch(`/api/activity${workspaceId ? `?workspace_id=${workspaceId}` : ""}`).then((r) =>
      json<Activity>(r),
    ),
  /** Stops the queue handing out new runs. In-flight work is left alone. */
  pauseQueue: (paused: boolean) =>
    post(`/api/queue/${paused ? "pause" : "resume"}`).then((r) =>
      json<{ paused: boolean }>(r),
    ),
  /** Dollars per day; null removes the cap. */
  setBudget: (capUsd: number | null) =>
    post("/api/queue/budget", { cap_usd: capUsd }).then((r) =>
      json<{ capUsd: number | null }>(r),
    ),
  teamEstimate: (teamId: string) =>
    fetch(`/api/teams/${teamId}/estimate`).then((r) => json<TeamEstimate>(r)),

  // agents
  agents: (workspaceId: string) =>
    fetch(`/api/agents?workspace_id=${workspaceId}`).then((r) =>
      json<{ agents: Agent[] }>(r),
    ),
  createAgent: (body: Record<string, unknown>) =>
    post("/api/agents", body).then((r) => json<Agent>(r)),
  updateAgent: (id: string, body: Record<string, unknown>) =>
    patch(`/api/agents/${id}`, body).then((r) => json<Agent>(r)),
  deleteAgent: (id: string) => fetch(`/api/agents/${id}`, { method: "DELETE" }),
  agentMemories: (agentId: string) =>
    fetch(`/api/agents/${agentId}/memories`).then((r) =>
      json<{ memories: AgentMemory[] }>(r),
    ),
  forgetMemory: (id: string) =>
    fetch(`/api/agent-memories/${id}`, { method: "DELETE" }).then((r) =>
      json<{ deleted: boolean }>(r),
    ),
  generateAgents: (description: string, engine?: string) =>
    post("/api/agents/generate", { description, engine }).then((r) =>
      json<{ drafts: AgentDraft[] }>(r),
    ),

  // teams
  teams: (workspaceId: string) =>
    fetch(`/api/teams?workspace_id=${workspaceId}`).then((r) => json<{ teams: Team[] }>(r)),
  createTeam: (body: Record<string, unknown>) =>
    post("/api/teams", body).then((r) => json<Team>(r)),
  updateTeam: (id: string, body: Record<string, unknown>) =>
    patch(`/api/teams/${id}`, body).then((r) => json<Team>(r)),
  deleteTeam: (id: string) => fetch(`/api/teams/${id}`, { method: "DELETE" }),

  // workflows
  workflows: (projectId: string) =>
    fetch(`/api/workflows?project_id=${projectId}`).then((r) =>
      json<{ workflows: WorkflowDef[] }>(r),
    ),
  createWorkflow: (projectId: string, sourceYaml: string) =>
    post("/api/workflows", { project_id: projectId, source_yaml: sourceYaml }).then((r) =>
      json<WorkflowDef>(r),
    ),
  updateWorkflow: (id: string, sourceYaml: string) =>
    patch(`/api/workflows/${id}`, { source_yaml: sourceYaml }).then((r) =>
      json<WorkflowDef>(r),
    ),
  setWorkflowEnabled: (id: string, enabled: boolean) =>
    patch(`/api/workflows/${id}`, { enabled }).then((r) => json<WorkflowDef>(r)),
  setWorkflowCatchUp: (id: string, catchUp: "skip" | "run_once") =>
    patch(`/api/workflows/${id}`, { catch_up: catchUp }).then((r) => json<WorkflowDef>(r)),
  deleteWorkflow: (id: string) => fetch(`/api/workflows/${id}`, { method: "DELETE" }),
  saveWorkflowLayout: (id: string, layout: Record<string, { x: number; y: number }>) =>
    post(`/api/workflows/${id}/layout`, layout),
  runWorkflow: (id: string) =>
    post(`/api/workflows/${id}/run`).then((r) => json<{ runId: string }>(r)),
  syncWorkflows: (projectId: string) =>
    post(`/api/projects/${projectId}/workflows/sync`).then((r) =>
      json<{ imported: WorkflowDef[]; errors: { file: string; error: string }[]; note?: string }>(r),
    ),
  workflowRuns: (projectId: string) =>
    fetch(`/api/workflow-runs?project_id=${projectId}`).then((r) =>
      json<{ runs: WorkflowRun[] }>(r),
    ),
  runSteps: (runId: string) =>
    fetch(`/api/runs/${runId}/steps`).then((r) => json<{ steps: RunStep[] }>(r)),
  runTeam: (teamId: string, projectId: string, goal: string) =>
    post(`/api/teams/${teamId}/run`, { project_id: projectId, goal }).then((r) =>
      json<{ runId: string; workflowId: string; steps: number }>(r),
    ),

  // organizations
  runOrg: (teamId: string, projectId: string, goal: string, reviewPlan = false) =>
    post(`/api/teams/${teamId}/run-org`, {
      project_id: projectId,
      goal,
      review_plan: reviewPlan,
    }).then((r) => json<{ runId: string }>(r)),
  // Both answer with the run they just changed, so the caller can render the
  // new state instead of racing its own poll for it.
  approvePlan: (runId: string) =>
    post(`/api/org-runs/${runId}/plan/approve`).then((r) => json<OrgRunDetail>(r)),
  rejectPlan: (runId: string, reason?: string) =>
    post(`/api/org-runs/${runId}/plan/reject`, { reason }).then((r) =>
      json<OrgRunDetail>(r),
    ),
  updateAssignment: (
    runId: string,
    stepId: string,
    body: Partial<{
      title: string;
      brief: string;
      assignee: string;
      done_when: string[];
      position: number;
    }>,
  ) => patch(`/api/org-runs/${runId}/assignments/${stepId}`, body).then(json),
  dropAssignment: (runId: string, stepId: string) =>
    fetch(`/api/org-runs/${runId}/assignments/${stepId}`, { method: "DELETE" }).then(json),
  orgRuns: (opts: { projectId?: string; workspaceId?: string }) => {
    const params = new URLSearchParams();
    if (opts.projectId) params.set("project_id", opts.projectId);
    if (opts.workspaceId) params.set("workspace_id", opts.workspaceId);
    return fetch(`/api/org-runs?${params}`).then((r) =>
      json<{ runs: OrgRunSummary[] }>(r),
    );
  },
  orgRun: (runId: string) =>
    fetch(`/api/org-runs/${runId}`).then((r) => json<OrgRunDetail>(r)),

  // project files (read-only viewer)
  files: (projectId: string, path?: string) =>
    fetch(
      `/api/projects/${projectId}/files${path ? `?path=${encodeURIComponent(path)}` : ""}`,
    ).then((r) => json<FileListing>(r)),
  file: (projectId: string, path: string) =>
    fetch(`/api/projects/${projectId}/file?path=${encodeURIComponent(path)}`).then((r) =>
      json<FileContent>(r),
    ),

  // attachments
  uploadAttachments: (projectId: string, files: File[]) => {
    const form = new FormData();
    for (const f of files) form.append("files", f);
    return postForm(`/api/projects/${projectId}/attachments`, form).then((r) =>
      json<{ attachments: Attachment[] }>(r),
    );
  },
  /** Only works while unclaimed — 409 once a task or message owns it. */
  deleteAttachment: (id: string) =>
    fetch(`/api/attachments/${id}`, { method: "DELETE" }).then((r) =>
      json<{ deleted: boolean }>(r),
    ),
  taskAttachments: (taskId: string) =>
    fetch(`/api/tasks/${taskId}/attachments`).then((r) =>
      json<{ attachments: Attachment[] }>(r),
    ),
  attachmentUrl: (id: string) => `/api/attachments/${id}`,

  /** Recursive filename search, for the @-mention picker. */
  searchFiles: (projectId: string, q: string) =>
    fetch(`/api/projects/${projectId}/files/search?q=${encodeURIComponent(q)}`).then((r) =>
      json<{ files: { path: string; name: string }[]; truncated: boolean }>(r),
    ),

  // global search
  search: (workspaceId: string, q: string) =>
    fetch(`/api/search?workspace_id=${workspaceId}&q=${encodeURIComponent(q)}`).then((r) =>
      json<SearchResults>(r),
    ),

  // chat
  openChat: (projectId: string) =>
    post(`/api/projects/${projectId}/chats`).then((r) => json<{ id: string }>(r)),
  chats: (projectId: string) =>
    fetch(`/api/projects/${projectId}/chats`).then((r) =>
      json<{ chats: ChatSummary[] }>(r),
    ),
  newChat: (projectId: string) =>
    post(`/api/projects/${projectId}/chats/new`).then((r) => json<{ id: string }>(r)),
  renameChat: (chatId: string, title: string) =>
    patch(`/api/chats/${chatId}`, { title }).then(json),
  // Throws on 409 when a turn is still running, so the UI can surface why.
  deleteChat: (chatId: string) =>
    fetch(`/api/chats/${chatId}`, { method: "DELETE" }).then((r) =>
      json<{ deleted: boolean }>(r),
    ),
  chatMessages: (chatId: string) =>
    fetch(`/api/chats/${chatId}/messages`).then((r) =>
      json<{ messages: ChatMessage[]; activeRunId: string | null }>(r),
    ),
  // Options object rather than trailing positionals: two optional args in a
  // row is exactly how a caller silently passes an engine as attachment ids.
  sendChat: (
    chatId: string,
    content: string,
    opts: {
      attachmentIds?: string[];
      engine?: string;
      modelTier?: Tier;
      effort?: Effort | null;
    } = {},
  ) =>
    post(`/api/chats/${chatId}/messages`, {
      content,
      engine: opts.engine,
      model_tier: opts.modelTier,
      effort: opts.effort ?? undefined,
      attachment_ids: opts.attachmentIds ?? [],
    }).then((r) => json<{ messageId: string; runId: string }>(r)),
};

export const tierColor: Record<Tier, string> = {
  easy: "var(--color-tier-easy)",
  medium: "var(--color-tier-medium)",
  complex: "var(--color-tier-complex)",
};

export const tierSoft: Record<Tier, string> = {
  easy: "var(--color-tier-easy-soft)",
  medium: "var(--color-tier-medium-soft)",
  complex: "var(--color-tier-complex-soft)",
};

// Tier → model labels are no longer a constant: the mapping is a user
// setting, so a baked-in label would name a model the run isn't using.
// See `useTierModel` in lib/models.
