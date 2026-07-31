/// <reference types="vite/client" />

/**
 * Types for the pieces of Monaco we actually import.
 *
 * `monaco-editor` ships types for its package root, but we import ESM
 * sub-paths so the TypeScript language service (which the root pulls in) stays
 * out of the bundle — see `components/editor/CodeEditor.tsx`. Those sub-paths
 * have no `.d.ts` of their own, so they are declared here as what they are:
 * the same API surface, minus the parts we chose not to load.
 *
 * This file deliberately has no top-level `import`/`export`. That keeps it a
 * global script, so the `declare module` blocks below are ambient declarations
 * of paths that have no types — rather than augmentations of modules that must
 * already exist, which is what they become the moment this becomes a module.
 */
declare module "monaco-editor/esm/vs/editor/edcore.main" {
  export * from "monaco-editor";
}

declare module "monaco-editor/esm/vs/basic-languages/monaco.contribution";
declare module "monaco-editor/esm/vs/language/json/monaco.contribution";

/**
 * How Monaco asks for its workers.
 *
 * Not in `lib.dom`, because it is Monaco's own global rather than a web
 * standard. `label` is the language id — `json`, `css`, `typescript` — or
 * `editorWorkerService` for the one every editor needs.
 */
interface Window {
  MonacoEnvironment?: {
    getWorker: (workerId: string, label: string) => Worker;
  };
}
