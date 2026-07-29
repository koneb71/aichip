import { Extension, Editor, Range } from "@tiptap/core";
import Suggestion from "@tiptap/suggestion";
import { menuRenderer, MenuItem } from "./suggestionMenu";

/**
 * `/` inserts a block.
 *
 * The toolbar can already do all of this, so the value is not the capability —
 * it is not having to move your hands. Halfway through a sentence, reaching for
 * a toolbar means losing the thought; `/table` does not.
 *
 * The placeholder text has promised this since the editor was written, which is
 * a good reason to actually build it: a UI that advertises a feature it does not
 * have teaches people to distrust the rest of it.
 */
export interface SlashCommand extends MenuItem {
  hint: string;
  run: (editor: Editor, range: Range) => void;
}

/** A command's `run` replaces the typed `/query` before doing its work. */
const at = (editor: Editor, range: Range) => editor.chain().focus().deleteRange(range);

export const SLASH_COMMANDS: SlashCommand[] = [
  {
    id: "h2",
    label: "Heading",
    hint: "Section title",
    run: (e, r) => at(e, r).toggleHeading({ level: 2 }).run(),
  },
  {
    id: "h3",
    label: "Subheading",
    hint: "Inside a section",
    run: (e, r) => at(e, r).toggleHeading({ level: 3 }).run(),
  },
  {
    id: "bullet",
    label: "Bulleted list",
    hint: "Unordered points",
    run: (e, r) => at(e, r).toggleBulletList().run(),
  },
  {
    id: "ordered",
    label: "Numbered list",
    hint: "Ordered steps",
    run: (e, r) => at(e, r).toggleOrderedList().run(),
  },
  {
    id: "task",
    label: "Checklist",
    hint: "Things to tick off",
    run: (e, r) => at(e, r).toggleTaskList().run(),
  },
  {
    id: "quote",
    label: "Quote",
    hint: "Set text apart",
    run: (e, r) => at(e, r).toggleBlockquote().run(),
  },
  {
    id: "code",
    label: "Code block",
    hint: "Syntax highlighted",
    run: (e, r) => at(e, r).toggleCodeBlock().run(),
  },
  {
    id: "details",
    label: "Toggle",
    hint: "Collapsible section",
    run: (e, r) => at(e, r).setDetails().run(),
  },
  {
    id: "table",
    label: "Table",
    // Three columns is the honest default for a keyboard-driven insert, and
    // the table bar adds rows and columns once it exists. Someone who wants a
    // specific shape up front uses the toolbar's grid.
    hint: "3 columns, add more after",
    run: (e, r) => at(e, r).insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run(),
  },
  {
    id: "rule",
    label: "Divider",
    hint: "A horizontal line",
    run: (e, r) => at(e, r).setHorizontalRule().run(),
  },
];

export const SlashMenu = Extension.create({
  name: "slashMenu",
  addProseMirrorPlugins() {
    return [
      Suggestion<SlashCommand>({
        editor: this.editor,
        char: "/",
        // Only at the start of an empty-ish block. Without this, a URL or a
        // date in the middle of a sentence opens a block menu, which is
        // startling and makes people stop typing slashes.
        allow: ({ state, range }) => {
          const $from = state.doc.resolve(range.from);
          return $from.parent.type.name === "paragraph" && range.from - $from.start() <= 1;
        },
        items: ({ query }) => {
          const q = query.trim().toLowerCase();
          return SLASH_COMMANDS.filter(
            (c) => !q || c.label.toLowerCase().includes(q) || c.id.includes(q),
          );
        },
        command: ({ editor, range, props }) => props.run(editor, range),
        render: menuRenderer,
      }),
    ];
  },
});
