import { useCallback, useEffect, useRef } from "react";
import { EditorContent, useEditor, Editor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import Image from "@tiptap/extension-image";
import Youtube from "@tiptap/extension-youtube";
import Placeholder from "@tiptap/extension-placeholder";
import Highlight from "@tiptap/extension-highlight";
import TextAlign from "@tiptap/extension-text-align";
import Superscript from "@tiptap/extension-superscript";
import Subscript from "@tiptap/extension-subscript";
import CharacterCount from "@tiptap/extension-character-count";
import CodeBlockLowlight from "@tiptap/extension-code-block-lowlight";
import { TaskList, TaskItem } from "@tiptap/extension-list";
import { Details, DetailsContent, DetailsSummary } from "@tiptap/extension-details";
import { Table, TableRow, TableCell, TableHeader } from "@tiptap/extension-table";
import type { EditorView } from "@tiptap/pm/view";
import { createLowlight, common } from "lowlight";
import { pageLinkMention } from "./pageLink";
import { SlashMenu } from "./slashMenu";
import { Toolbar } from "./Toolbar";

/**
 * The page editor: TipTap, which is MIT.
 *
 * It replaced TinyMCE for two reasons, and the licence was only the second one.
 * TinyMCE 8 self-hosted is GPLv2-or-later, a copyleft obligation this MIT
 * project should not hand its users — and separately, its self-hosted setup
 * rendered an *invisible* editor, because `skin: false` tells it to apply no
 * skin at all and the toolbar came out as unstyled transparent divs.
 *
 * TipTap emits HTML natively, so nothing behind it changed: the same sanitiser,
 * the same text projection, the same diff, the same search index. A
 * markdown-first editor would have meant rewriting all four.
 *
 * One rule governs which extensions can be added here: **whatever they emit has
 * to survive `kb::sanitize`.** An extension whose markup gets stripped on save
 * is worse than not having it, because the feature appears to work right up
 * until the page reloads. Checklists needed a sanitiser change for exactly
 * this reason; mentions render as plain links for it.
 */
const lowlight = createLowlight(common);

export function RichTextEditor({
  value,
  onChange,
  workspaceId,
  onAssetUploaded,
}: {
  value: string;
  onChange: (html: string) => void;
  workspaceId: string;
  /** Uploads are claimed by the page when it saves, so the caller has to
   *  remember which ones happened while the editor was open. */
  onAssetUploaded: (id: string) => void;
}) {
  // Refs, not closures: the extensions are constructed once, and a callback
  // that captured the first render's workspace would keep uploading there
  // after a switch.
  const ws = useRef(workspaceId);
  ws.current = workspaceId;
  const notify = useRef(onAssetUploaded);
  notify.current = onAssetUploaded;

  const upload = useCallback(async (file: File): Promise<string> => {
    const form = new FormData();
    form.append("file", file);
    const res = await fetch(`/api/kb/assets?workspace_id=${ws.current}`, {
      method: "POST",
      body: form,
    });
    if (!res.ok) throw new Error((await res.text()) || "upload failed");
    const [asset] = (await res.json()).assets;
    notify.current(asset.id);
    return asset.url;
  }, []);

  const editor = useEditor({
    extensions: [
      StarterKit.configure({
        link: { openOnClick: false, HTMLAttributes: { rel: "noopener noreferrer" } },
        // Replaced below by the highlighting version.
        codeBlock: false,
      }),
      CodeBlockLowlight.configure({ lowlight }),
      Image.configure({ HTMLAttributes: { class: "kb-image" } }),
      // Paste a watch URL and it becomes a player. The server's sanitiser is
      // what decides which hosts survive being saved, so this is convenience
      // rather than the security boundary.
      //
      // Extended to also recognise a *bare* iframe: the extension writes
      // `<div data-youtube-video><iframe>`, the sanitiser strips the wrapper,
      // and without this the embed parses as an unknown node and disappears
      // the second time the page is opened for editing.
      Youtube.extend({
        parseHTML() {
          return [
            { tag: "div[data-youtube-video] iframe" },
            {
              tag: "iframe",
              getAttrs: (el) => {
                const src = (el as HTMLElement).getAttribute("src") ?? "";
                return /youtube(-nocookie)?\.com|youtu\.be/.test(src) && { src };
              },
            },
          ];
        },
      }).configure({ controls: true, nocookie: true, width: 640, height: 360 }),
      Highlight.configure({ multicolor: false }),
      Superscript,
      Subscript,
      TextAlign.configure({ types: ["heading", "paragraph"] }),
      TaskList,
      // The tick is an attribute drawn in CSS rather than a real checkbox.
      // The default markup wraps each item in `<label><input type=checkbox>`,
      // and admitting form controls into stored documents to render a glyph is
      // a bad trade — so the sanitiser allows `data-checked` and nothing else.
      TaskItem.extend({
        renderHTML({ node, HTMLAttributes }) {
          return [
            "li",
            { ...HTMLAttributes, "data-checked": node.attrs.checked ? "true" : "false" },
            0,
          ];
        },
      }).configure({ nested: true }),
      Details.configure({ persist: true }),
      DetailsSummary,
      DetailsContent,
      Table.configure({ resizable: true }),
      TableRow,
      TableHeader,
      TableCell,
      CharacterCount,
      Placeholder.configure({
        placeholder: ({ node }) =>
          node.type.name === "heading"
            ? "Heading"
            : "Write, or press / for blocks and @ to link a page",
      }),
      pageLinkMention(ws),
      SlashMenu,
    ],
    content: value,
    editorProps: {
      attributes: { class: "kb kb-editor focus:outline-none" },
      // Dropping and pasting images uploads them rather than embedding base64 —
      // a pasted screenshot as a data URL would bloat the row, defeat the diff,
      // and be stripped by the sanitiser anyway.
      handleDrop: (view, event) => {
        const files = Array.from(event.dataTransfer?.files ?? []).filter((f) =>
          f.type.startsWith("image/"),
        );
        if (!files.length) return false;
        event.preventDefault();
        const at = view.posAtCoords({ left: event.clientX, top: event.clientY })?.pos;
        files.forEach((f) => insertUploaded(view, upload, f, at));
        return true;
      },
      handlePaste: (view, event) => {
        const files = Array.from(event.clipboardData?.files ?? []).filter((f) =>
          f.type.startsWith("image/"),
        );
        if (!files.length) return false;
        event.preventDefault();
        files.forEach((f) => insertUploaded(view, upload, f));
        return true;
      },
    },
    onUpdate: ({ editor }) => onChange(editor.getHTML()),
  });

  // Only push external changes in. Writing `value` back on every render would
  // fight the user's cursor on every keystroke.
  useEffect(() => {
    if (!editor) return;
    if (value !== editor.getHTML()) {
      editor.commands.setContent(value, { emitUpdate: false });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value, editor]);

  if (!editor) return <div className="p-4 text-sm text-ink-dim">Loading the editor…</div>;

  return (
    <div className="rounded-xl border border-line bg-panel">
      <Toolbar editor={editor} upload={upload} />
      <EditorContent editor={editor} />
      <StatusBar editor={editor} />
    </div>
  );
}

/** Upload, then put the resulting URL where the drop happened. */
async function insertUploaded(
  view: EditorView,
  upload: (f: File) => Promise<string>,
  file: File,
  at?: number,
) {
  try {
    const src = await upload(file);
    const node = view.state.schema.nodes.image.create({ src, alt: file.name });
    const tr = view.state.tr;
    view.dispatch(at === undefined ? tr.replaceSelectionWith(node) : tr.insert(at, node));
  } catch (e) {
    alert(`Upload failed: ${String(e).replace(/^Error:\s*/, "")}`);
  }
}

/**
 * Length, and a nudge when a page gets long enough that agents will only see
 * part of it — the prompt budget is real and invisible otherwise.
 */
function StatusBar({ editor }: { editor: Editor }) {
  const chars = editor.storage.characterCount.characters();
  const words = editor.storage.characterCount.words();
  // Matches MAX_PAGE_CHARS in crates/aichip-core/src/kb/mod.rs.
  const over = chars > 6000;
  return (
    <div className="flex items-center gap-3 border-t border-line px-3 py-1.5 text-[10px] text-ink-dim">
      <span>
        {words} {words === 1 ? "word" : "words"} · {chars.toLocaleString()} characters
      </span>
      {over && (
        <span className="text-amber-700">
          past ~6,000 characters an agent attached to this page sees only the start
        </span>
      )}
    </div>
  );
}
