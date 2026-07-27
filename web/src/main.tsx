import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter, Route, Routes } from "react-router-dom";
import { WorkspaceProvider } from "./lib/workspace";
import AppShell from "./AppShell";
import HomePage from "./pages/HomePage";
import ProjectsPage from "./pages/ProjectsPage";
import ProjectPage from "./pages/ProjectPage";
import AgentsPage from "./pages/AgentsPage";
import TeamsPage from "./pages/TeamsPage";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <WorkspaceProvider>
      <BrowserRouter>
        <Routes>
          <Route path="/" element={<AppShell />}>
            <Route index element={<HomePage />} />
            <Route path="projects" element={<ProjectsPage />} />
            <Route path="projects/:projectId" element={<ProjectPage />} />
            <Route path="agents" element={<AgentsPage />} />
            <Route path="teams" element={<TeamsPage />} />
          </Route>
        </Routes>
      </BrowserRouter>
    </WorkspaceProvider>
  </React.StrictMode>,
);
