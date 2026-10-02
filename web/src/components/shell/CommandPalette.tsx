import { Command } from "cmdk";
import * as RD from "@radix-ui/react-dialog";
import { useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { ArrowRight, FolderPlus, Moon, Pause, Play, Search, Sun, Monitor } from "lucide-react";
import { api, SearchResults } from "../../lib/api";
import { NAV, searchRows, type SearchRow } from "../../lib/nav";
import { useTheme } from "../../lib/theme";
import { useWorkspace } from "../../lib/workspace";
import { useActivity } from "../../lib/activity";
import { Kbd } from "../ui/Badge";
import { toast } from "../ui/Toast";

/**
 * ⌘K: go anywhere, find anything, do the common things — without a mouse.
 *
 * Three groups. Pages are filtered locally by cmdk as you type; search hits
 * come from the server once there are two characters, debounced, with a stale
 * guard so a slow early answer can't overwrite a later one; actions are the
 * handful of things people reach for from anywhere.
 */

const EMPTY: SearchResults = { projects: [], tasks: [], agents: [], teams: [], workflows: [], goals: [] };

export function CommandPalette({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const navigate = useNavigate();
  const { active } = useWorkspace();
  const { choice, setChoice } = useTheme();
  const { activity, refresh } = useActivity();
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<SearchRow[]>([]);

  useEffect(() => {
    if (!open) setQuery("");
  }, [open]);

  useEffect(() => {
    const q = query.trim();
    if (!active || q.length < 2) {
      setHits([]);
      return;
    }
    let stale = false;
    const t = setTimeout(() => {
      api
        .search(active.id, q)
        .then((r) => !stale && setHits(searchRows(r)))
        .catch(() => !stale && setHits(searchRows(EMPTY)));
    }, 160);
    return () => {
      stale = true;
      clearTimeout(t);
    };
  }, [active, query]);

  const go = (to: string) => {
    onOpenChange(false);
    navigate(to);
  };

  const paused = activity?.gate.state === "paused";
  const groups = [...new Set(hits.map((h) => h.group))];

  return (
    <RD.Root open={open} onOpenChange={onOpenChange}>
      <RD.Portal>
        <RD.Overlay className="fixed inset-0 z-40 bg-[color-mix(in_oklab,black_35%,transparent)] data-[state=open]:animate-[fade-in_var(--dur-fast)_var(--ease-out-soft)]" />
        <RD.Content
          className="fixed left-1/2 top-[14vh] z-50 w-[calc(100vw-32px)] max-w-[580px] -translate-x-1/2 overflow-hidden rounded-xl border border-border bg-raised text-fg shadow-[var(--shadow-lg)] outline-none data-[state=open]:animate-[dialog-in_var(--dur-base)_var(--ease-out-soft)]"
          aria-describedby={undefined}
        >
          <RD.Title className="sr-only">Command palette</RD.Title>
          {/* Server hits are already filtered by the server; cmdk filters only the local lists. */}
          <Command label="Command palette" loop>
            <div className="flex items-center gap-2 border-b border-border px-3.5">
              <Search className="size-4 shrink-0 text-fg-subtle" />
              <Command.Input
                value={query}
                onValueChange={setQuery}
                placeholder="Search projects, cards, agents… or type a command"
                className="h-11 w-full bg-transparent text-sm text-fg outline-none placeholder:text-fg-subtle"
              />
              <Kbd>esc</Kbd>
            </div>
            <Command.List className="max-h-[min(420px,60vh)] overflow-y-auto p-1.5 [&_[cmdk-group-heading]]:px-2 [&_[cmdk-group-heading]]:pb-1 [&_[cmdk-group-heading]]:pt-2 [&_[cmdk-group-heading]]:text-[11px] [&_[cmdk-group-heading]]:font-medium [&_[cmdk-group-heading]]:text-fg-subtle">
              <Command.Empty className="px-3 py-6 text-center text-[13px] text-fg-muted">
                Nothing matches “{query.trim()}”.
              </Command.Empty>

              {groups.map((g) => (
                <Command.Group key={g} heading={g} forceMount>
                  {hits
                    .filter((h) => h.group === g)
                    .map((h) => (
                      <Item key={h.id} value={`${h.id} ${h.label} ${query}`} onSelect={() => go(h.to)} sub={h.sublabel}>
                        {h.label}
                      </Item>
                    ))}
                </Command.Group>
              ))}

              <Command.Group heading="Go to">
                {NAV.map((n) => {
                  const Icon = n.icon;
                  return (
                    <Item key={n.to} value={`go ${n.label} ${(n.keywords ?? []).join(" ")}`} onSelect={() => go(n.to)} icon={<Icon className="size-4" />}>
                      {n.label}
                    </Item>
                  );
                })}
              </Command.Group>

              <Command.Group heading="Actions">
                <Item value="new project create" onSelect={() => go("/projects?new=1")} icon={<FolderPlus className="size-4" />}>
                  New project
                </Item>
                <Item
                  value={`${paused ? "resume" : "pause"} queue`}
                  icon={paused ? <Play className="size-4" /> : <Pause className="size-4" />}
                  onSelect={async () => {
                    onOpenChange(false);
                    try {
                      await api.pauseQueue(!paused);
                      refresh?.();
                      toast(paused ? "Queue resumed" : "Queue paused", { tone: "success" });
                    } catch (e) {
                      toast("Could not change the queue", { tone: "danger", body: String(e) });
                    }
                  }}
                >
                  {paused ? "Resume the queue" : "Pause the queue"}
                </Item>
                {(["light", "dark", "system"] as const)
                  .filter((c) => c !== choice)
                  .map((c) => (
                    <Item
                      key={c}
                      value={`theme ${c} appearance`}
                      icon={c === "light" ? <Sun className="size-4" /> : c === "dark" ? <Moon className="size-4" /> : <Monitor className="size-4" />}
                      onSelect={() => {
                        setChoice(c);
                        onOpenChange(false);
                      }}
                    >
                      {c === "system" ? "Match the system theme" : `Switch to ${c} theme`}
                    </Item>
                  ))}
              </Command.Group>
            </Command.List>
          </Command>
        </RD.Content>
      </RD.Portal>
    </RD.Root>
  );
}

function Item({
  children,
  value,
  onSelect,
  icon,
  sub,
}: {
  children: React.ReactNode;
  value: string;
  onSelect: () => void;
  icon?: React.ReactNode;
  sub?: string;
}) {
  return (
    <Command.Item
      value={value}
      onSelect={onSelect}
      className="group flex h-9 cursor-default items-center gap-2.5 rounded-md px-2 text-[13px] text-fg outline-none data-[selected=true]:bg-panel-2"
    >
      <span className="grid size-4 shrink-0 place-items-center text-fg-muted">{icon ?? <ArrowRight className="size-3.5" />}</span>
      <span className="min-w-0 flex-1 truncate">{children}</span>
      {sub && <span className="max-w-[40%] shrink-0 truncate text-xs text-fg-subtle">{sub}</span>}
    </Command.Item>
  );
}
