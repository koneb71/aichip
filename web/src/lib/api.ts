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
  /** Set when the latest run was an organization run — opens the team room. */
  orgRunId: string | null;
  runId: string | null;
  runStatus: string | null;
  costUsd: number | null;
  model: string | null;
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
  permissionPreset: string;
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

export interface OrgAssignment {
  id: string;
  key: string;
  status: string;
  assignee: string | null;
  title: string | null;
  brief: string | null;
  output: string | null;
  dependsOn: string[];
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
  moveTask: (taskId: string, body: { board_column?: string; position?: number }) =>
    patch(`/api/tasks/${taskId}`, body).then((r) =>
      json<{ moved: boolean; runId: string | null }>(r),
    ),
  taskComments: (taskId: string) =>
    fetch(`/api/tasks/${taskId}/comments`).then((r) =>
      json<{ comments: TaskComment[]; pendingReplies: number }>(r),
    ),
  postComment: (taskId: string, content: string, engine?: string) =>
    post(`/api/tasks/${taskId}/comments`, { content, engine }).then((r) =>
      json<{ id: string; runIds: string[] }>(r),
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
  pendingPermissions: (runId: string) =>
    fetch(`/api/runs/${runId}/pending-permissions`).then((r) =>
      json<{ pending: PendingPermission[] }>(r),
    ),
  resolvePermission: (requestId: string, allowed: boolean) =>
    post(`/api/permissions/${requestId}/resolve`, { allowed }),

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
  runOrg: (teamId: string, projectId: string, goal: string) =>
    post(`/api/teams/${teamId}/run-org`, { project_id: projectId, goal }).then((r) =>
      json<{ runId: string }>(r),
    ),
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
    opts: { attachmentIds?: string[]; engine?: string } = {},
  ) =>
    post(`/api/chats/${chatId}/messages`, {
      content,
      engine: opts.engine,
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

export const tierModel: Record<Tier, string> = {
  easy: "Sonnet 5",
  medium: "Opus 5",
  complex: "Fable 5",
};
