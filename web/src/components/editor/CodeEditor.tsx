import { useEffect, useRef } from "react";
import * as monaco from "monaco-editor/esm/vs/editor/edcore.main";
import "monaco-editor/esm/vs/basic-languages/monaco.contribution";
import "monaco-editor/esm/vs/language/json/monaco.contribution";
import editorWorker from "monaco-editor/esm/vs/editor/editor.worker?worker";
import jsonWorker from "monaco-editor/esm/vs/language/json/json.worker?worker";

/**
 * Monaco, self-hosted and loaded only when someone opens a file.
 *
 * **This is the only module that imports monaco**, and it must stay that way:
 * `FilesPanel` reaches it through `React.lazy`, so the ~830 KB gzip never lands
 * in the main chunk. An import of this file from anywhere eager silently undoes
 * that, and the only symptom is a bigger `index-*.js`.
 *
 * Assembled from parts rather than `import "monaco-editor"`, which is core plus
 * every language service:
 *
 * - `edcore.main` — the editor and all its contributions: find/replace,
 *   folding, multi-cursor, bracket matching, the minimap.
 * - `basic-languages` — ~90 grammars. Syntax, comments and brackets, which is
 *   what "supports a language" means for reading and editing a file.
 * - the JSON service — real validation and formatting, and small.
 *
 * The **TypeScript/JavaScript service is deliberately absent.** It is not
 * merely large (~1.4 MB gzip); it would be actively wrong here. Its worker sees
 * one file with no `tsconfig`, no `node_modules` and no path aliases, so it
 * reports `Cannot find module` on every import and `Cannot find name 'React'`
 * on correct code. An editor that puts fifty false errors on a file that
 * compiles is worse than one that puts none. `basic-languages` already colours
 * `.ts` and `.tsx` properly.
 */

// Workers come through Vite's `?worker` suffix, which bundles each as its own
// asset and hands back a constructor. No CDN, no publicPath, no copy step —
// offline by construction, which is the point for a local-first app.
//
// `editor.worker` is needed even with no language services at all: link
// detection and diff computation run in it.
self.MonacoEnvironment = {
  getWorker(_id: string, label: string) {
    return label === "json" ? new jsonWorker() : new editorWorker();
  },
};

/** The light theme, for anything that embeds the editor in the app's own
 *  chrome. */
monaco.editor.defineTheme("aichip", {
  base: "vs",
  inherit: true,
  colors: {
    "editor.background": "#ffffff",
    "editorLineNumber.foreground": "#9ca3af",
    "editorLineNumber.activeForeground": "#4b5563",
    "editor.lineHighlightBackground": "#f6f6f7",
    "editorIndentGuide.background1": "#ececee",
  },
  rules: [],
});

/** The IDE theme: vs-dark tuned to the Files tab's shell, which paints the
 *  editor-adjacent chrome in the same palette. */
monaco.editor.defineTheme("aichip-dark", {
  base: "vs-dark",
  inherit: true,
  colors: {
    "editor.background": "#1e1e1e",
    "editorLineNumber.foreground": "#6e7681",
    "editorLineNumber.activeForeground": "#cccccc",
    "editor.lineHighlightBackground": "#2a2d2e",
  },
  rules: [],
});

export default function CodeEditor({
  value,
  language,
  path,
  readOnly,
  dark,
  onChange,
  onSave,
  onCursor,
}: {
  value: string;
  language: string;
  /** Identity for the model, so undo history and cursor survive a round trip. */
  path: string;
  readOnly?: boolean;
  /** The Files tab's IDE shell is dark; everything else is light. */
  dark?: boolean;
  onChange: (next: string) => void;
  onSave: () => void;
  /** For a status bar's Ln, Col. */
  onCursor?: (line: number, col: number) => void;
}) {
  const host = useRef<HTMLDivElement>(null);
  const editor = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);
  // Held in a ref so the command registered once always calls the newest one.
  const save = useRef(onSave);
  save.current = onSave;
  // Both held in refs so the listener and command registered once always reach
  // the newest closure, without tearing the editor down on every render.
  const onChangeRef = useRef(onChange);
  onChangeRef.current = onChange;
  const onCursorRef = useRef(onCursor);
  onCursorRef.current = onCursor;
  // Every model this editor created, disposed together on unmount. Not on
  // tab switch: the whole point of a model per path is that undo history and
  // cursor survive coming back.
  const models = useRef(new Set<monaco.editor.ITextModel>());

  useEffect(() => {
    if (!host.current) return;
    const instance = monaco.editor.create(host.current, {
      theme: dark ? "aichip-dark" : "aichip",
      automaticLayout: false,
      minimap: { enabled: true },
      scrollBeyondLastLine: false,
      fontSize: 12,
      renderWhitespace: "selection",
      tabSize: 2,
    });
    editor.current = instance;

    const sub = instance.onDidChangeModelContent(() => {
      onChangeRef.current(instance.getValue());
    });
    const cursorSub = instance.onDidChangeCursorPosition((e) => {
      onCursorRef.current?.(e.position.lineNumber, e.position.column);
    });

    // Registered on the editor rather than on `window`: a global keydown would
    // swallow Save while someone is typing in the chat composer or a wiki page.
    instance.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () =>
      save.current(),
    );

    // Monaco does not size itself, and this sits inside a flex column.
    const resize = new ResizeObserver(() => instance.layout());
    resize.observe(host.current);

    return () => {
      resize.disconnect();
      sub.dispose();
      cursorSub.dispose();
      // Models outlive the editor unless disposed, and one per opened file
      // adds up over a browsing session. All of them, not just the mounted
      // one — tab switches leave the rest alive on purpose.
      instance.setModel(null);
      for (const m of models.current) m.dispose();
      models.current.clear();
      instance.dispose();
      editor.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // A model per path, so switching files and coming back keeps undo history
  // and the cursor where it was.
  useEffect(() => {
    const instance = editor.current;
    if (!instance) return;
    const uri = monaco.Uri.parse(`aichip:/${path}`);
    const existing = monaco.editor.getModel(uri);
    const model = existing ?? monaco.editor.createModel(value, language, uri);
    models.current.add(model);
    if (existing && existing.getValue() !== value) {
      // The file changed underneath us — a reload or a conflict resolution.
      // `setValue` rather than recreating, so undo still reaches back.
      existing.setValue(value);
    }
    monaco.editor.setModelLanguage(model, language);
    if (instance.getModel() !== model) {
      // The previous model stays alive: switching tabs and coming back keeps
      // undo history and the cursor. Unmount disposes the lot.
      instance.setModel(model);
    }
  }, [path, language, value]);

  useEffect(() => {
    editor.current?.updateOptions({ readOnly: !!readOnly });
  }, [readOnly]);

  return <div ref={host} className="h-full w-full" />;
}
