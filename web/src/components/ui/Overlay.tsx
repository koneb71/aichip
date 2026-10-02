import * as RM from "@radix-ui/react-dropdown-menu";
import * as RP from "@radix-ui/react-popover";
import * as RTip from "@radix-ui/react-tooltip";
import type { ReactNode } from "react";
import { cn } from "./cn";

/**
 * Tooltip, menu and popover — the small floating things. Portalled, so a
 * clipped scroll container never cuts one off, and keyboard-reachable, which
 * the six invisible click-away layers they replace were not.
 */

const FLOAT =
  "z-50 rounded-lg border border-border bg-raised text-fg shadow-[var(--shadow-md)] outline-none data-[state=open]:animate-[pop-in_var(--dur-fast)_var(--ease-out-soft)]";

export function TooltipProvider({ children }: { children: ReactNode }) {
  return <RTip.Provider delayDuration={350} skipDelayDuration={150}>{children}</RTip.Provider>;
}

export function Tooltip({
  content,
  children,
  side = "top",
}: {
  content: ReactNode;
  children: ReactNode;
  side?: "top" | "bottom" | "left" | "right";
}) {
  if (!content) return <>{children}</>;
  return (
    <RTip.Root>
      <RTip.Trigger asChild>{children}</RTip.Trigger>
      <RTip.Portal>
        <RTip.Content
          side={side}
          sideOffset={6}
          className="z-50 max-w-xs rounded-md bg-fg px-2 py-1 text-[11px] leading-snug text-bg shadow-[var(--shadow-md)] data-[state=delayed-open]:animate-[pop-in_var(--dur-fast)_var(--ease-out-soft)]"
        >
          {content}
        </RTip.Content>
      </RTip.Portal>
    </RTip.Root>
  );
}

export interface MenuItem {
  label: ReactNode;
  onSelect: () => void;
  icon?: ReactNode;
  shortcut?: ReactNode;
  danger?: boolean;
  disabled?: boolean;
}

export function Menu({
  trigger,
  items,
  align = "end",
  label,
}: {
  trigger: ReactNode;
  /** `null` draws a separator. */
  items: Array<MenuItem | null>;
  align?: "start" | "end";
  label?: string;
}) {
  return (
    <RM.Root>
      <RM.Trigger asChild>{trigger}</RM.Trigger>
      <RM.Portal>
        <RM.Content align={align} sideOffset={6} className={cn(FLOAT, "min-w-[180px] p-1")}>
          {label && <RM.Label className="px-2 pb-1 pt-1.5 text-[11px] font-medium text-fg-subtle">{label}</RM.Label>}
          {items.map((it, i) =>
            it === null ? (
              <RM.Separator key={`s${i}`} className="my-1 h-px bg-border" />
            ) : (
              <RM.Item
                key={i}
                disabled={it.disabled}
                onSelect={it.onSelect}
                className={cn(
                  "flex h-7 cursor-default select-none items-center gap-2 rounded-[5px] px-2 text-[13px] outline-none data-[disabled]:opacity-50 data-[highlighted]:bg-panel-2",
                  it.danger ? "text-danger-fg" : "text-fg",
                )}
              >
                {it.icon && <span className="grid size-4 place-items-center text-fg-muted">{it.icon}</span>}
                <span className="flex-1 truncate">{it.label}</span>
                {it.shortcut && <span className="text-[11px] text-fg-subtle">{it.shortcut}</span>}
              </RM.Item>
            ),
          )}
        </RM.Content>
      </RM.Portal>
    </RM.Root>
  );
}

export function Popover({
  trigger,
  children,
  open,
  onOpenChange,
  align = "start",
  className,
}: {
  trigger: ReactNode;
  children: ReactNode;
  open?: boolean;
  onOpenChange?: (o: boolean) => void;
  align?: "start" | "center" | "end";
  className?: string;
}) {
  return (
    <RP.Root open={open} onOpenChange={onOpenChange}>
      <RP.Trigger asChild>{trigger}</RP.Trigger>
      <RP.Portal>
        <RP.Content align={align} sideOffset={6} className={cn(FLOAT, "p-3", className)}>
          {children}
        </RP.Content>
      </RP.Portal>
    </RP.Root>
  );
}
