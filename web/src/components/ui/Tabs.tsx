import * as RT from "@radix-ui/react-tabs";
import type { ReactNode } from "react";
import { cn } from "./cn";

/**
 * Tabs with the real roles: arrow keys move between them, and each panel is
 * labelled by its tab. The project page's hand-rolled tab strip had neither.
 */

export interface TabDef<T extends string> {
  value: T;
  label: ReactNode;
  /** A count or dot after the label. */
  badge?: ReactNode;
}

export function Tabs<T extends string>({
  value,
  onValueChange,
  tabs,
  children,
  className,
  listClassName,
  variant = "underline",
}: {
  value: T;
  onValueChange: (v: T) => void;
  tabs: TabDef<T>[];
  children?: ReactNode;
  className?: string;
  listClassName?: string;
  /** `underline` for page sections; `pill` for a compact switch inside a panel. */
  variant?: "underline" | "pill";
}) {
  return (
    <RT.Root value={value} onValueChange={(v) => onValueChange(v as T)} className={cn("flex min-h-0 flex-col", className)}>
      <RT.List
        className={cn(
          "flex shrink-0 items-center gap-1 overflow-x-auto",
          variant === "underline" ? "border-b border-border px-1" : "w-fit rounded-md bg-panel-2 p-0.5",
          listClassName,
        )}
      >
        {tabs.map((t) => (
          <RT.Trigger
            key={t.value}
            value={t.value}
            className={cn(
              "ring-focus inline-flex shrink-0 items-center gap-1.5 whitespace-nowrap text-[13px] font-medium text-fg-muted transition-colors hover:text-fg",
              variant === "underline"
                ? "-mb-px h-9 border-b-2 border-transparent px-2.5 data-[state=active]:border-accent data-[state=active]:text-fg"
                : "h-6 rounded-[5px] px-2.5 text-xs data-[state=active]:bg-panel data-[state=active]:text-fg data-[state=active]:shadow-[var(--shadow-xs)]",
            )}
          >
            {t.label}
            {t.badge}
          </RT.Trigger>
        ))}
      </RT.List>
      {children}
    </RT.Root>
  );
}

export function TabPanel({ value, children, className }: { value: string; children: ReactNode; className?: string }) {
  return (
    <RT.Content value={value} className={cn("min-h-0 flex-1 outline-none", className)}>
      {children}
    </RT.Content>
  );
}
