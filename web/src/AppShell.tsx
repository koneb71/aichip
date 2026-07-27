import { Outlet } from "react-router-dom";
import { Sidebar } from "./components/sidebar/Sidebar";

export default function AppShell() {
  return (
    <div className="grid h-full grid-cols-[240px_1fr]">
      <Sidebar />
      <main className="min-h-0 min-w-0 overflow-hidden">
        <Outlet />
      </main>
    </div>
  );
}
