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
}

export interface Task {
  id: string;
  title: string;
  modelTier: Tier;
  boardColumn: "backlog" | "running" | "review" | "done";
  branch: string | null;
  projectId: string;
  agentId: string | null;
  agentName: string | null;
  agentColor: string | null;
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
  pattern: "pipeline" | "debate" | "swarm";
  definition: { members?: { agent_id: string; role?: string }[] };
}

export interface ChatMessage {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
  runId: string | null;
  ts: string;
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
  addProject: (workspaceId: string, path: string) =>
    post("/api/projects", { workspace_id: workspaceId, path }).then((r) =>
      json<{ id: string; name: string }>(r),
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
    engine?: string;
  }) => post("/api/tasks", body).then((r) => json<{ id: string; runId: string | null }>(r)),
  startTask: (taskId: string) =>
    post(`/api/tasks/${taskId}/start`).then((r) => json<{ runId: string }>(r)),
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

  // chat
  openChat: (projectId: string) =>
    post(`/api/projects/${projectId}/chats`).then((r) => json<{ id: string }>(r)),
  chatMessages: (chatId: string) =>
    fetch(`/api/chats/${chatId}/messages`).then((r) =>
      json<{ messages: ChatMessage[]; activeRunId: string | null }>(r),
    ),
  sendChat: (chatId: string, content: string, engine?: string) =>
    post(`/api/chats/${chatId}/messages`, { content, engine }).then((r) =>
      json<{ messageId: string; runId: string }>(r),
    ),
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
