import { useRef } from "react";
import { motion } from "framer-motion";
import { api, Attachment, ATTACHMENT_ACCEPT } from "../lib/api";
import { PendingAttachment } from "../lib/useAttachments";

function glyph(kind: string): string {
  if (kind === "pdf") return "📄";
  if (kind === "image") return "🖼";
  return "📝";
}

export function humanSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** Composer chips for files being attached, plus the paperclip button. */
export function AttachmentBar({
  items,
  onAdd,
  onRemove,
  full,
  disabled,
}: {
  items: PendingAttachment[];
  onAdd: (files: FileList) => void;
  onRemove: (localId: string) => void;
  full: boolean;
  disabled?: boolean;
}) {
  const input = useRef<HTMLInputElement>(null);

  return (
    <div className="flex flex-wrap items-center gap-1.5">
      <input
        ref={input}
        type="file"
        multiple
        accept={ATTACHMENT_ACCEPT}
        className="hidden"
        onChange={(e) => {
          if (e.target.files) onAdd(e.target.files);
          // Reset so picking the same file twice still fires onChange.
          e.target.value = "";
        }}
      />
      <button
        type="button"
        onClick={() => input.current?.click()}
        disabled={disabled || full}
        title={full ? "Attachment limit reached" : "Attach images, PDFs or text files"}
        className="rounded-lg px-1.5 py-1 text-sm text-ink-dim hover:bg-panel-2 hover:text-ink disabled:opacity-40"
      >
        📎
      </button>

      {items.map((item) => (
        <motion.div
          key={item.localId}
          initial={{ opacity: 0, scale: 0.96 }}
          animate={{ opacity: 1, scale: 1 }}
          title={item.error ?? `${item.name} · ${humanSize(item.size)}`}
          className={`flex min-w-0 max-w-[190px] items-center gap-1.5 rounded-lg border bg-panel-2 py-1 pl-1 pr-1.5 text-xs ${
            item.status === "error" ? "border-danger text-danger" : "border-line text-ink-dim"
          }`}
        >
          {item.previewUrl ? (
            <img src={item.previewUrl} alt="" className="h-6 w-6 rounded object-cover" />
          ) : (
            <span className="flex h-6 w-6 items-center justify-center">
              {glyph(item.remote?.kind ?? "text")}
            </span>
          )}
          <span className="min-w-0 flex-1 truncate">{item.name}</span>
          {item.status === "uploading" && (
            <motion.span
              className="h-2 w-2 shrink-0 rounded-full bg-accent"
              animate={{ opacity: [0.3, 1, 0.3] }}
              transition={{ repeat: Infinity, duration: 1.1 }}
            />
          )}
          <button
            type="button"
            onClick={() => onRemove(item.localId)}
            className="shrink-0 px-0.5 hover:text-danger"
            title="Remove"
          >
            ✕
          </button>
        </motion.div>
      ))}
    </div>
  );
}

/** Read-only rendering for attachments already sent — chat history, task drawer. */
export function AttachmentList({ attachments }: { attachments: Attachment[] }) {
  if (!attachments?.length) return null;
  return (
    <div className="mb-1.5 flex flex-col gap-1.5">
      {attachments.map((a) =>
        a.kind === "image" ? (
          <a key={a.id} href={api.attachmentUrl(a.id)} target="_blank" rel="noreferrer">
            <img
              src={api.attachmentUrl(a.id)}
              alt={a.filename}
              className="max-h-48 rounded-lg border border-line object-contain"
            />
          </a>
        ) : (
          <a
            key={a.id}
            href={api.attachmentUrl(a.id)}
            target="_blank"
            rel="noreferrer"
            className="flex items-center gap-1.5 rounded-lg border border-line bg-panel px-2 py-1 text-xs text-ink-dim hover:bg-panel-2"
          >
            <span>{glyph(a.kind)}</span>
            <span className="min-w-0 flex-1 truncate">{a.filename}</span>
            <span className="shrink-0 text-[10px] opacity-70">{humanSize(a.size)}</span>
          </a>
        ),
      )}
    </div>
  );
}
