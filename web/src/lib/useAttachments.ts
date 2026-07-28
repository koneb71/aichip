import { useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  Attachment,
  ATTACHMENT_ACCEPT,
  MAX_ATTACHMENTS,
  MAX_ATTACHMENT_BYTES,
} from "./api";

/** A file in the composer: uploading, uploaded, or rejected. */
export interface PendingAttachment {
  localId: string;
  name: string;
  size: number;
  status: "uploading" | "ready" | "error";
  remote?: Attachment;
  error?: string;
  /** Object URL for image previews. Revoked on removal and unmount. */
  previewUrl?: string;
}

const ALLOWED_EXTS = ATTACHMENT_ACCEPT.split(",").map((e) => e.slice(1));

function extOf(name: string): string {
  const i = name.lastIndexOf(".");
  return i === -1 ? "" : name.slice(i + 1).toLowerCase();
}

/** Same rules the server enforces, applied early so obvious rejects cost no round trip. */
function preCheck(file: File): string | null {
  if (!ALLOWED_EXTS.includes(extOf(file.name))) {
    return `${file.name}: unsupported type`;
  }
  if (file.size > MAX_ATTACHMENT_BYTES) {
    return `${file.name} is larger than ${MAX_ATTACHMENT_BYTES / 1024 / 1024} MB`;
  }
  return null;
}

/**
 * Composer attachment state, shared by the chat panel and the new-task modal.
 *
 * Uploads happen immediately and per-file, so one rejection never poisons the
 * batch and the ids are ready by the time the user submits.
 */
export function useAttachments(projectId: string) {
  const [items, setItems] = useState<PendingAttachment[]>([]);
  const [dragging, setDragging] = useState(false);
  // Read by the unmount cleanup, which must not re-run when items change.
  const itemsRef = useRef<PendingAttachment[]>([]);
  itemsRef.current = items;

  const revoke = (item: PendingAttachment) => {
    if (item.previewUrl) URL.revokeObjectURL(item.previewUrl);
  };

  const add = useCallback(
    (incoming: FileList | File[]) => {
      const files = Array.from(incoming);
      if (!files.length) return;

      setItems((prev) => {
        const room = MAX_ATTACHMENTS - prev.length;
        const accepted = files.slice(0, Math.max(0, room));
        const next = accepted.map((file) => {
          const localId = `${file.name}-${file.size}-${crypto.randomUUID()}`;
          const problem = preCheck(file);
          const previewUrl = file.type.startsWith("image/")
            ? URL.createObjectURL(file)
            : undefined;

          if (!problem) {
            api
              .uploadAttachments(projectId, [file])
              .then((r) =>
                setItems((cur) =>
                  cur.map((i) =>
                    i.localId === localId
                      ? { ...i, status: "ready" as const, remote: r.attachments[0] }
                      : i,
                  ),
                ),
              )
              .catch((e) =>
                setItems((cur) =>
                  cur.map((i) =>
                    i.localId === localId
                      ? { ...i, status: "error" as const, error: String(e) }
                      : i,
                  ),
                ),
              );
          }

          return {
            localId,
            name: file.name,
            size: file.size,
            status: problem ? ("error" as const) : ("uploading" as const),
            error: problem ?? undefined,
            previewUrl,
          };
        });
        return [...prev, ...next];
      });
    },
    [projectId],
  );

  const remove = useCallback((localId: string) => {
    setItems((prev) => {
      const gone = prev.find((i) => i.localId === localId);
      if (gone) {
        revoke(gone);
        // Best effort: an already-claimed row 409s, which is fine.
        if (gone.remote) api.deleteAttachment(gone.remote.id).catch(() => {});
      }
      return prev.filter((i) => i.localId !== localId);
    });
  }, []);

  /** After a successful submit: the rows are claimed, so only drop local state. */
  const clear = useCallback(() => {
    setItems((prev) => {
      prev.forEach(revoke);
      return [];
    });
  }, []);

  // Discard anything still unclaimed when the composer goes away. Best effort
  // only — the server-side sweeper is the real backstop.
  useEffect(() => {
    return () => {
      itemsRef.current.forEach((item) => {
        revoke(item);
        if (item.remote) api.deleteAttachment(item.remote.id).catch(() => {});
      });
    };
  }, []);

  const onPaste = useCallback(
    (e: React.ClipboardEvent) => {
      const files = Array.from(e.clipboardData?.files ?? []);
      // Only swallow the event when there is actually a file — otherwise
      // pasting text into the composer would stop working.
      if (!files.length) return;
      e.preventDefault();
      add(files);
    },
    [add],
  );

  const dropProps = {
    onDragOver: (e: React.DragEvent) => {
      if (!e.dataTransfer?.types.includes("Files")) return;
      e.preventDefault();
      setDragging(true);
    },
    onDragLeave: (e: React.DragEvent) => {
      // Ignore bubbling from children, or the overlay flickers.
      if (e.currentTarget.contains(e.relatedTarget as Node)) return;
      setDragging(false);
    },
    onDrop: (e: React.DragEvent) => {
      if (!e.dataTransfer?.files.length) return;
      e.preventDefault();
      setDragging(false);
      add(e.dataTransfer.files);
    },
  };

  return {
    items,
    /** Ids ready to submit. */
    ids: items.filter((i) => i.remote).map((i) => i.remote!.id),
    /** True while any upload is still in flight — submit should wait. */
    busy: items.some((i) => i.status === "uploading"),
    full: items.length >= MAX_ATTACHMENTS,
    add,
    remove,
    clear,
    onPaste,
    dropProps,
    dragging,
  };
}
