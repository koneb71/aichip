import {
  createContext,
  ReactNode,
  useCallback,
  useContext,
  useEffect,
  useState,
} from "react";
import { api, Workspace } from "./api";

interface WorkspaceCtx {
  workspaces: Workspace[];
  active: Workspace | null;
  setActive: (id: string) => void;
  refresh: () => Promise<void>;
}

const Ctx = createContext<WorkspaceCtx>({
  workspaces: [],
  active: null,
  setActive: () => {},
  refresh: async () => {},
});

const STORAGE_KEY = "aichip.workspace";

export function WorkspaceProvider({ children }: { children: ReactNode }) {
  const [workspaces, setWorkspaces] = useState<Workspace[]>([]);
  const [activeId, setActiveId] = useState<string | null>(
    localStorage.getItem(STORAGE_KEY),
  );

  const refresh = useCallback(async () => {
    const { workspaces } = await api.workspaces();
    setWorkspaces(workspaces);
    setActiveId((current) =>
      current && workspaces.some((w) => w.id === current)
        ? current
        : (workspaces[0]?.id ?? null),
    );
  }, []);

  useEffect(() => {
    refresh().catch(() => {});
  }, [refresh]);

  const setActive = useCallback((id: string) => {
    localStorage.setItem(STORAGE_KEY, id);
    setActiveId(id);
  }, []);

  const active = workspaces.find((w) => w.id === activeId) ?? null;
  return (
    <Ctx.Provider value={{ workspaces, active, setActive, refresh }}>
      {children}
    </Ctx.Provider>
  );
}

export const useWorkspace = () => useContext(Ctx);
