import { useLocation, useNavigate } from "react-router-dom";
import { Bell, Menu as MenuIcon, Monitor, Moon, Plus, Search, Sun, FolderPlus, Bot } from "lucide-react";
import { useInbox } from "../../lib/inbox";
import { sectionFor } from "../../lib/nav";
import { useCrumbItems } from "../../lib/crumbs";
import { useTheme } from "../../lib/theme";
import { Breadcrumbs } from "../ui/Layout";
import { Button, IconButton } from "../ui/Button";
import { Kbd } from "../ui/Badge";
import { Menu, Tooltip } from "../ui/Overlay";

/**
 * The bar above every page: where you are, and the four things you might
 * want from anywhere — search, theme, something new, and (on a phone) the
 * navigation drawer.
 */
export function TopBar({ onOpenPalette, onOpenNav }: { onOpenPalette: () => void; onOpenNav?: () => void }) {
  const { pathname } = useLocation();
  const navigate = useNavigate();
  const section = sectionFor(pathname);
  const trail = useCrumbItems();
  const { choice, cycle } = useTheme();
  const { unread } = useInbox();

  const themeLabel = choice === "system" ? "Theme: system" : choice === "light" ? "Theme: light" : "Theme: dark";
  const ThemeIcon = choice === "system" ? Monitor : choice === "light" ? Sun : Moon;

  return (
    <header className="flex h-12 shrink-0 items-center gap-2 border-b border-border bg-panel px-3 sm:px-4">
      {onOpenNav && (
        <IconButton label="Open navigation" onClick={onOpenNav}>
          <MenuIcon className="size-4" />
        </IconButton>
      )}
      <Breadcrumbs
        className="min-w-0 flex-1"
        items={[
          ...(section ? [{ label: section.label, to: section.to }] : [{ label: "aichip" }]),
          ...trail,
        ]}
      />

      <button
        type="button"
        onClick={onOpenPalette}
        className="ring-focus hidden h-7 w-56 items-center gap-2 rounded-md border border-border bg-bg px-2 text-xs text-fg-subtle hover:border-border-strong hover:text-fg-muted md:flex"
      >
        <Search className="size-3.5" />
        <span className="flex-1 text-left">Search or jump to…</span>
        <Kbd>⌘K</Kbd>
      </button>
      <IconButton label="Search" onClick={onOpenPalette} className="md:hidden">
        <Search className="size-4" />
      </IconButton>

      <Tooltip content={unread ? `${unread} waiting on you` : "Inbox"}>
        <IconButton label={unread ? `Inbox, ${unread} unread` : "Inbox"} onClick={() => navigate("/inbox")} className="relative">
          <Bell className="size-4" />
          {unread > 0 && <span className="absolute right-1 top-1 size-1.5 rounded-full bg-accent ring-2 ring-panel" />}
        </IconButton>
      </Tooltip>

      <Tooltip content={`${themeLabel} — click to change`}>
        <IconButton label={themeLabel} onClick={cycle}>
          <ThemeIcon className="size-4" />
        </IconButton>
      </Tooltip>

      <Menu
        align="end"
        trigger={
          <Button variant="primary" size="sm" icon={<Plus className="size-3.5" />}>
            <span className="hidden sm:inline">New</span>
          </Button>
        }
        items={[
          { label: "Project", icon: <FolderPlus className="size-3.5" />, onSelect: () => navigate("/projects?new=1") },
          { label: "Agent", icon: <Bot className="size-3.5" />, onSelect: () => navigate("/agents?new=1") },
        ]}
      />
    </header>
  );
}
