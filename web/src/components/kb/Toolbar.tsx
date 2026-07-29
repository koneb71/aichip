import { useState } from "react";
import { Editor } from "@tiptap/react";
import { TableSizePicker } from "./TableSizePicker";

/**
 * The toolbar.
 *
 * Grouped by what a thing *is* — marks, blocks, alignment, inserts, history —
 * rather than by how often it's used, because a toolbar people scan needs a
 * shape they can learn once. Table controls appear only inside a table: they
 * are meaningless everywhere else and would be eight permanently-dead buttons.
 */
export function Toolbar({
  editor,
  upload,
}: {
  editor: Editor;
  upload: (f: File) => Promise<string>;
}) {
  const pickImage = () => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "image/*";
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;
      try {
        editor.chain().focus().setImage({ src: await upload(file), alt: file.name }).run();
      } catch (e) {
        alert(`Upload failed: ${String(e).replace(/^Error:\s*/, "")}`);
      }
    };
    input.click();
  };

  const attach = () => {
    const input = document.createElement("input");
    input.type = "file";
    input.onchange = async () => {
      const file = input.files?.[0];
      if (!file) return;
      try {
        const url = await upload(file);
        // A file that isn't an image becomes a download link rather than a
        // broken image — the upload endpoint takes PDFs and zips too.
        editor
          .chain()
          .focus()
          .insertContent(`<a href="${url}">${escapeHtml(file.name)}</a>`)
          .run();
      } catch (e) {
        alert(`Upload failed: ${String(e).replace(/^Error:\s*/, "")}`);
      }
    };
    input.click();
  };

  const embed = () => {
    const url = prompt("Paste a YouTube link");
    if (url) editor.commands.setYoutubeVideo({ src: url });
  };

  const link = () => {
    const previous = editor.getAttributes("link").href ?? "";
    const url = prompt("Link to (leave empty to remove)", previous);
    if (url === null) return;
    if (url === "") editor.chain().focus().unsetLink().run();
    else editor.chain().focus().setLink({ href: url }).run();
  };

  return (
    <div className="sticky top-0 z-10 rounded-t-xl border-b border-line bg-panel">
      <div className="flex flex-wrap items-center gap-0.5 px-2 py-1.5">
        <BlockPicker editor={editor} />
        <Divider />

        <Group>
          <T ed={editor} on="bold" go={(c) => c.toggleBold()} title="Bold  ⌘B" cls="font-bold">
            B
          </T>
          <T ed={editor} on="italic" go={(c) => c.toggleItalic()} title="Italic  ⌘I" cls="italic">
            I
          </T>
          <T ed={editor} on="underline" go={(c) => c.toggleUnderline()} title="Underline  ⌘U" cls="underline">
            U
          </T>
          <T ed={editor} on="strike" go={(c) => c.toggleStrike()} title="Strikethrough" cls="line-through">
            S
          </T>
          <T ed={editor} on="highlight" go={(c) => c.toggleHighlight()} title="Highlight">
            ▨
          </T>
          <T ed={editor} on="code" go={(c) => c.toggleCode()} title="Inline code">
            ‹›
          </T>
          <T ed={editor} on="superscript" go={(c) => c.toggleSuperscript()} title="Superscript">
            x²
          </T>
          <T ed={editor} on="subscript" go={(c) => c.toggleSubscript()} title="Subscript">
            x₂
          </T>
        </Group>
        <Divider />

        <Group>
          <T ed={editor} on="bulletList" go={(c) => c.toggleBulletList()} title="Bulleted list">
            •
          </T>
          <T ed={editor} on="orderedList" go={(c) => c.toggleOrderedList()} title="Numbered list">
            1.
          </T>
          <T ed={editor} on="taskList" go={(c) => c.toggleTaskList()} title="Checklist">
            ☑
          </T>
          <T ed={editor} on="blockquote" go={(c) => c.toggleBlockquote()} title="Quote">
            ❝
          </T>
        </Group>
        <Divider />

        <Group>
          <T
            ed={editor}
            on={{ name: "paragraph", attrs: { textAlign: "left" } }}
            go={(c) => c.setTextAlign("left")}
            title="Align left"
          >
            ⇤
          </T>
          <T
            ed={editor}
            on={{ name: "paragraph", attrs: { textAlign: "center" } }}
            go={(c) => c.setTextAlign("center")}
            title="Centre"
          >
            ⇔
          </T>
          <T
            ed={editor}
            on={{ name: "paragraph", attrs: { textAlign: "right" } }}
            go={(c) => c.setTextAlign("right")}
            title="Align right"
          >
            ⇥
          </T>
        </Group>
        <Divider />

        <Group>
          <P onClick={link} title="Link  ⌘K">🔗</P>
          <P onClick={pickImage} title="Insert an image">🖼</P>
          <P onClick={attach} title="Attach a file">📎</P>
          <P onClick={embed} title="Embed a video">▶</P>
          <TableSizePicker
            onPick={(rows, cols, withHeaderRow) =>
              editor.chain().focus().insertTable({ rows, cols, withHeaderRow }).run()
            }
          />
          <P onClick={() => editor.chain().focus().setHorizontalRule().run()} title="Divider">
            ―
          </P>
        </Group>
        <Divider />

        <Group>
          <P
            onClick={() => editor.chain().focus().unsetAllMarks().clearNodes().run()}
            title="Clear formatting"
          >
            ⌫
          </P>
          <P
            onClick={() => editor.chain().focus().undo().run()}
            title="Undo  ⌘Z"
            disabled={!editor.can().undo()}
          >
            ↶
          </P>
          <P
            onClick={() => editor.chain().focus().redo().run()}
            title="Redo  ⇧⌘Z"
            disabled={!editor.can().redo()}
          >
            ↷
          </P>
        </Group>

        <span className="ml-auto hidden pr-1 text-[10px] text-ink-dim sm:block">
          / for blocks · @ to link a page
        </span>
      </div>

      {editor.isActive("table") && <TableBar editor={editor} />}
    </div>
  );
}

/**
 * Table editing, shown only when the caret is in one.
 *
 * Inserting a table was previously the only thing you could do to it — no way
 * to add a row, delete a column, or get rid of it. A table you cannot change is
 * a table you have to delete and retype.
 */
function TableBar({ editor }: { editor: Editor }) {
  const c = () => editor.chain().focus();
  return (
    <div className="flex flex-wrap items-center gap-0.5 border-t border-line bg-panel-2 px-2 py-1">
      <span className="mr-1 text-[10px] uppercase tracking-wide text-ink-dim">Table</span>
      <P onClick={() => c().addRowBefore().run()} title="Add a row above">↑+</P>
      <P onClick={() => c().addRowAfter().run()} title="Add a row below">↓+</P>
      <P onClick={() => c().deleteRow().run()} title="Delete this row">−row</P>
      <Divider />
      <P onClick={() => c().addColumnBefore().run()} title="Add a column left">←+</P>
      <P onClick={() => c().addColumnAfter().run()} title="Add a column right">→+</P>
      <P onClick={() => c().deleteColumn().run()} title="Delete this column">−col</P>
      <Divider />
      <P onClick={() => c().toggleHeaderRow().run()} title="Toggle the header row">header</P>
      <P onClick={() => c().mergeOrSplit().run()} title="Merge or split cells">merge</P>
      <P onClick={() => c().deleteTable().run()} title="Delete the whole table">delete</P>
    </div>
  );
}

/** Block type as a menu, so the common case is one click and one glance. */
function BlockPicker({ editor }: { editor: Editor }) {
  const [open, setOpen] = useState(false);
  const current = editor.isActive("heading", { level: 1 })
    ? "Title"
    : editor.isActive("heading", { level: 2 })
      ? "Heading"
      : editor.isActive("heading", { level: 3 })
        ? "Subheading"
        : editor.isActive("codeBlock")
          ? "Code"
          : editor.isActive("blockquote")
            ? "Quote"
            : "Text";

  const choose = (fn: () => void) => {
    fn();
    setOpen(false);
  };
  const c = () => editor.chain().focus();

  return (
    <div className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex items-center gap-1 rounded-md px-2 py-1 text-xs text-ink-dim hover:bg-panel-2 hover:text-ink"
      >
        {current} <span className="text-[8px]">▾</span>
      </button>
      {open && (
        <>
          {/* Click-away, so the menu doesn't strand itself open. */}
          <div className="fixed inset-0 z-10" onClick={() => setOpen(false)} />
          <div className="card-shadow absolute left-0 top-full z-20 mt-1 w-44 rounded-xl border border-line bg-panel p-1">
            {[
              ["Text", () => c().setParagraph().run()],
              ["Title", () => c().toggleHeading({ level: 1 }).run()],
              ["Heading", () => c().toggleHeading({ level: 2 }).run()],
              ["Subheading", () => c().toggleHeading({ level: 3 }).run()],
              ["Quote", () => c().toggleBlockquote().run()],
              ["Code", () => c().toggleCodeBlock().run()],
              ["Toggle", () => c().setDetails().run()],
            ].map(([label, fn]) => (
              <button
                key={label as string}
                type="button"
                onClick={() => choose(fn as () => void)}
                className={`block w-full rounded-lg px-2 py-1.5 text-left text-xs hover:bg-panel-2 ${
                  current === label ? "text-accent" : ""
                }`}
              >
                {label as string}
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

const Group = ({ children }: { children: React.ReactNode }) => (
  <div className="flex items-center gap-0.5">{children}</div>
);
const Divider = () => <span className="mx-1 h-4 w-px shrink-0 bg-line" />;

/** A plain action button. */
function P({
  onClick,
  title,
  disabled,
  children,
}: {
  onClick: () => void;
  title: string;
  disabled?: boolean;
  children: React.ReactNode;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      disabled={disabled}
      className="rounded-md px-2 py-1 text-xs text-ink-dim hover:bg-panel-2 hover:text-ink disabled:opacity-30 disabled:hover:bg-transparent"
    >
      {children}
    </button>
  );
}

/** A toggle that knows whether its mark or node is active. */
function T({
  ed,
  on,
  go,
  title,
  cls = "",
  children,
}: {
  ed: Editor;
  on: string | { name: string; attrs: Record<string, unknown> };
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  go: (chain: any) => any;
  title: string;
  cls?: string;
  children: React.ReactNode;
}) {
  const active = typeof on === "string" ? ed.isActive(on) : ed.isActive(on.name, on.attrs);
  return (
    <button
      type="button"
      title={title}
      onClick={() => go(ed.chain().focus()).run()}
      className={`rounded-md px-2 py-1 text-xs ${cls} ${
        active ? "bg-accent/10 text-accent" : "text-ink-dim hover:bg-panel-2 hover:text-ink"
      }`}
    >
      {children}
    </button>
  );
}

function escapeHtml(s: string) {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}
