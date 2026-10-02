import * as RD from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "./cn";

/**
 * A modal dialog and a side sheet, both on Radix: focus is trapped and
 * returned, Escape closes, the page behind is inert and scroll-locked, and the
 * title is what a screen reader announces. The eighteen hand-rolled overlays
 * this replaces did none of that.
 *
 * `dismissible={false}` is for a form holding unsaved work: Escape and a click
 * outside then do nothing, because either is easy to do by accident and would
 * throw the work away. The close button and the form's own buttons still
 * close it — those are never an accident.
 */

const OVERLAY =
  "fixed inset-0 z-40 bg-[color-mix(in_oklab,black_42%,transparent)] backdrop-blur-[2px] data-[state=open]:animate-[fade-in_var(--dur-base)_var(--ease-out-soft)] data-[state=closed]:animate-[fade-out_var(--dur-fast)_var(--ease-out-soft)]";

export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  footer,
  width = 480,
  className,
  dismissible = true,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  children?: ReactNode;
  /** Buttons, right-aligned. */
  footer?: ReactNode;
  width?: number;
  className?: string;
  /** False: only the close button and the dialog's own buttons close it. */
  dismissible?: boolean;
}) {
  return (
    <RD.Root open={open} onOpenChange={onOpenChange}>
      <RD.Portal>
        <RD.Overlay className={OVERLAY} />
        <RD.Content
          onEscapeKeyDown={guard(dismissible)}
          onInteractOutside={guard(dismissible)}
          className={cn(
            "fixed left-1/2 top-[12vh] z-50 flex max-h-[76vh] w-[calc(100vw-32px)] -translate-x-1/2 flex-col overflow-hidden rounded-xl border border-border bg-raised text-fg shadow-[var(--shadow-lg)] outline-none data-[state=open]:animate-[dialog-in_var(--dur-base)_var(--ease-out-soft)]",
            className,
          )}
          style={{ maxWidth: width }}
        >
          <div className="flex items-start justify-between gap-4 border-b border-border px-5 py-3.5">
            <div className="min-w-0">
              <RD.Title className="text-[15px] font-semibold leading-snug">{title}</RD.Title>
              {description ? (
                <RD.Description className="mt-0.5 text-xs leading-relaxed text-fg-muted">{description}</RD.Description>
              ) : (
                <RD.Description className="sr-only">{typeof title === "string" ? title : "Dialog"}</RD.Description>
              )}
            </div>
            <RD.Close className="ring-focus -mr-1 grid size-7 shrink-0 place-items-center rounded-md text-fg-muted hover:bg-panel-2 hover:text-fg" aria-label="Close">
              <X className="size-4" />
            </RD.Close>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">{children}</div>
          {footer && <div className="flex items-center justify-end gap-2 border-t border-border bg-panel px-5 py-3">{footer}</div>}
        </RD.Content>
      </RD.Portal>
    </RD.Root>
  );
}

/** Cancels Escape and outside interaction while the work is not to be lost. */
function guard(dismissible: boolean) {
  return dismissible ? undefined : (e: Event) => e.preventDefault();
}

/** A panel that slides in from the right: a card, an agent, a run. */
export function Sheet({
  open,
  onOpenChange,
  title,
  description,
  actions,
  children,
  width = 560,
  className,
  dismissible = true,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  /** Header controls beside the close button. */
  actions?: ReactNode;
  children: ReactNode;
  width?: number;
  className?: string;
  /** False: only the close button and the sheet's own buttons close it. */
  dismissible?: boolean;
}) {
  return (
    <RD.Root open={open} onOpenChange={onOpenChange}>
      <RD.Portal>
        <RD.Overlay className={cn(OVERLAY, "bg-[color-mix(in_oklab,black_22%,transparent)] backdrop-blur-0")} />
        <RD.Content
          onEscapeKeyDown={guard(dismissible)}
          onInteractOutside={guard(dismissible)}
          className={cn(
            "fixed inset-y-0 right-0 z-50 flex w-full flex-col border-l border-border bg-panel text-fg shadow-[var(--shadow-lg)] outline-none data-[state=open]:animate-[sheet-in_var(--dur-base)_var(--ease-out-soft)]",
            className,
          )}
          style={{ maxWidth: width }}
        >
          <div className="flex items-start justify-between gap-3 border-b border-border px-4 py-3">
            <div className="min-w-0 flex-1">
              <RD.Title className="truncate text-[15px] font-semibold leading-snug">{title}</RD.Title>
              {description ? (
                <RD.Description className="mt-0.5 text-xs text-fg-muted">{description}</RD.Description>
              ) : (
                <RD.Description className="sr-only">{typeof title === "string" ? title : "Panel"}</RD.Description>
              )}
            </div>
            <div className="flex shrink-0 items-center gap-1">
              {actions}
              <RD.Close className="ring-focus grid size-7 place-items-center rounded-md text-fg-muted hover:bg-panel-2 hover:text-fg" aria-label="Close">
                <X className="size-4" />
              </RD.Close>
            </div>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto">{children}</div>
        </RD.Content>
      </RD.Portal>
    </RD.Root>
  );
}
