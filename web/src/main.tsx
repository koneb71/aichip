import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter, Route, Routes } from "react-router-dom";
import { WorkspaceProvider } from "./lib/workspace";
import { ActivityProvider } from "./lib/activity";
import { ModelsProvider } from "./lib/models";
import AppShell from "./AppShell";
import HomePage from "./pages/HomePage";
import ProjectsPage from "./pages/ProjectsPage";
import ProjectPage from "./pages/ProjectPage";
import AgentsPage from "./pages/AgentsPage";
import ActivityPage from "./pages/ActivityPage";
import ConnectionsPage from "./pages/ConnectionsPage";
import SettingsPage from "./pages/SettingsPage";
import TeamsPage from "./pages/TeamsPage";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <WorkspaceProvider>
      <ModelsProvider>
        <ActivityProvider>
        <BrowserRouter>
          <Routes>
            <Route path="/" element={<AppShell />}>
              <Route index element={<HomePage />} />
              <Route path="projects" element={<ProjectsPage />} />
              <Route path="projects/:projectId" element={<ProjectPage />} />
              <Route path="activity" element={<ActivityPage />} />
              <Route path="agents" element={<AgentsPage />} />
              <Route path="connections" element={<ConnectionsPage />} />
              <Route path="settings" element={<SettingsPage />} />
              <Route path="teams" element={<TeamsPage />} />
            </Route>
          </Routes>
        </BrowserRouter>
        </ActivityProvider>
      </ModelsProvider>
    </WorkspaceProvider>
  </React.StrictMode>,
);
