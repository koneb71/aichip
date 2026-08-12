import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter, Route, Routes } from "react-router-dom";
import { WorkspaceProvider } from "./lib/workspace";
import { ActivityProvider } from "./lib/activity";
import { ModelsProvider } from "./lib/models";
import { EnginesProvider } from "./lib/engines";
import AppShell from "./AppShell";
import HomePage from "./pages/HomePage";
import ChatPage from "./pages/ChatPage";
import ResearchPage from "./pages/ResearchPage";
import ProjectsPage from "./pages/ProjectsPage";
import ProjectPage from "./pages/ProjectPage";
import AgentsPage from "./pages/AgentsPage";
import SkillsPage from "./pages/SkillsPage";
import AppsPage from "./pages/AppsPage";
import AppPage from "./pages/AppPage";
import ActivityPage from "./pages/ActivityPage";
import ConnectionsPage from "./pages/ConnectionsPage";
import SettingsPage from "./pages/SettingsPage";
import TeamsPage from "./pages/TeamsPage";
import "./index.css";

// Reading a page must not download an editor. Only the edit route is lazy —
// the editor is far larger than any single page view, and a wiki is read far
// more often than it is written.
import KnowledgeLayout from "./pages/knowledge/KnowledgeLayout";
import KnowledgeHome from "./pages/knowledge/KnowledgeHome";
import PageView from "./pages/knowledge/PageView";
import PageHistory from "./pages/knowledge/PageHistory";
const PageEditor = React.lazy(() => import("./pages/knowledge/PageEditor"));

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <WorkspaceProvider>
      <EnginesProvider>
      <ModelsProvider>
        <ActivityProvider>
        <BrowserRouter>
          <Routes>
            <Route path="/" element={<AppShell />}>
              <Route index element={<HomePage />} />
              <Route path="chat" element={<ChatPage />} />
              <Route path="research" element={<ResearchPage />} />
              <Route path="research/:researchId" element={<ResearchPage />} />
              <Route path="projects" element={<ProjectsPage />} />
              <Route path="projects/:projectId" element={<ProjectPage />} />
              <Route path="activity" element={<ActivityPage />} />
              <Route path="agents" element={<AgentsPage />} />
              <Route path="skills" element={<SkillsPage />} />
              <Route path="apps" element={<AppsPage />} />
              <Route path="apps/:appId" element={<AppPage />} />
              <Route path="knowledge" element={<KnowledgeLayout />}>
                <Route index element={<KnowledgeHome />} />
                <Route path=":pageId" element={<PageView />} />
                <Route
                  path=":pageId/edit"
                  element={
                    <React.Suspense
                      fallback={
                        <div className="p-8 text-sm text-ink-dim">Loading the editor…</div>
                      }
                    >
                      <PageEditor />
                    </React.Suspense>
                  }
                />
                <Route path=":pageId/history" element={<PageHistory />} />
              </Route>
              <Route path="connections" element={<ConnectionsPage />} />
              <Route path="settings" element={<SettingsPage />} />
              <Route path="teams" element={<TeamsPage />} />
            </Route>
          </Routes>
        </BrowserRouter>
        </ActivityProvider>
      </ModelsProvider>
      </EnginesProvider>
    </WorkspaceProvider>
  </React.StrictMode>,
);
