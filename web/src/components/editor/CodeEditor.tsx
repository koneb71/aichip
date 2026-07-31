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

/** One theme. `index.css` has a single light `@theme` and no dark mode, so a
 *  switcher would have nothing to switch between. */
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

export default function CodeEditor({
  value,
  language,
  path,
  readOnly,
  onChange,
  onSave,
}: {
  value: string;
  language: string;
  /** Identity for the model, so undo history and cursor survive a round trip. */
  path: string;
  readOnly?: boolean;
  onChange: (next: string) => void;
  onSave: () => void;
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

  useEffect(() => {
    if (!host.current) return;
    const instance = monaco.editor.create(host.current, {
      theme: "aichip",
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
      // Models outlive the editor unless disposed, and one per opened file
      // adds up over a browsing session.
      instance.getModel()?.dispose();
      instance.dispose();
      editor.current = null;
    };
  }, []);

  // A model per path, so switching files and coming back keeps undo history
  // and the cursor where it was.
  useEffect(() => {
    const instance = editor.current;
    if (!instance) return;
    const uri = monaco.Uri.parse(`aichip:/${path}`);
    const existing = monaco.editor.getModel(uri);
    const model = existing ?? monaco.editor.createModel(value, language, uri);
    if (existing && existing.getValue() !== value) {
      // The file changed underneath us — a reload or a conflict resolution.
      // `setValue` rather than recreating, so undo still reaches back.
      existing.setValue(value);
    }
    monaco.editor.setModelLanguage(model, language);
    if (instance.getModel() !== model) {
      const previous = instance.getModel();
      instance.setModel(model);
      if (previous && previous !== model) previous.dispose();
    }
  }, [path, language, value]);

  useEffect(() => {
    editor.current?.updateOptions({ readOnly: !!readOnly });
  }, [readOnly]);

  return <div ref={host} className="h-full w-full" />;
}
