import { CheckCircle2, Info, TriangleAlert, X, XCircle } from "lucide-react";
import { useEffect, useSyncExternalStore } from "react";
import { cn } from "./cn";

/**
 * Brief confirmations: "Copied", "Budget saved", "Merge refused — see the card".
 *
 * A module-level store rather than a context, so anything — a page, an API
 * helper, a socket handler — can say something without being inside a
 * provider. One viewport, bottom-right, announced politely to screen readers.
 */

export type ToastTone = "neutral" | "success" | "warning" | "danger";

interface ToastItem {
  id: number;
  tone: ToastTone;
  title: string;
  body?: string;
}

let items: ToastItem[] = [];
let seq = 0;
const listeners = new Set<() => void>();

function emit() {
  for (const l of listeners) l();
}

export function toast(title: string, opts: { tone?: ToastTone; body?: string; ms?: number } = {}) {
  const id = ++seq;
  items = [...items.slice(-3), { id, tone: opts.tone ?? "neutral", title, body: opts.body }];
  emit();
  const ms = opts.ms ?? (opts.tone === "danger" ? 7000 : 3500);
  window.setTimeout(() => dismiss(id), ms);
  return id;
}

export function dismiss(id: number) {
  items = items.filter((t) => t.id !== id);
  emit();
}

function subscribe(l: () => void) {
  listeners.add(l);
  return () => listeners.delete(l);
}

const ICON = {
  neutral: <Info className="size-4 text-info-fg" />,
  success: <CheckCircle2 className="size-4 text-success-fg" />,
  warning: <TriangleAlert className="size-4 text-warning-fg" />,
  danger: <XCircle className="size-4 text-danger-fg" />,
};

export function Toaster() {
  const list = useSyncExternalStore(subscribe, () => items);
  // Escape clears them all — a stack of stale confirmations is noise.
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      if (e.key === "Escape" && items.length) {
        items = [];
        emit();
      }
    };
    window.addEventListener("keydown", on);
    return () => window.removeEventListener("keydown", on);
  }, []);
  return (
    <div
      aria-live="polite"
      className="pointer-events-none fixed bottom-4 right-4 z-[60] flex w-[min(360px,calc(100vw-32px))] flex-col gap-2"
    >
      {list.map((t) => (
        <div
          key={t.id}
          role={t.tone === "danger" ? "alert" : "status"}
          className={cn(
            "pointer-events-auto flex items-start gap-2.5 rounded-lg border border-border bg-raised px-3 py-2.5 text-fg shadow-[var(--shadow-lg)]",
            "animate-[toast-in_var(--dur-base)_var(--ease-out-soft)]",
          )}
        >
          <span className="mt-px">{ICON[t.tone]}</span>
          <div className="min-w-0 flex-1">
            <div className="text-[13px] font-medium leading-snug">{t.title}</div>
            {t.body && <div className="mt-0.5 text-xs leading-relaxed text-fg-muted">{t.body}</div>}
          </div>
          <button
            type="button"
            onClick={() => dismiss(t.id)}
            className="ring-focus -mr-1 grid size-5 place-items-center rounded text-fg-subtle hover:text-fg"
            aria-label="Dismiss"
          >
            <X className="size-3.5" />
          </button>
        </div>
      ))}
    </div>
  );
}
