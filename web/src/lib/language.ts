/**
 * Which language a file is, from its name alone.
 *
 * A plain map rather than asking Monaco, for two reasons: it keeps language
 * selection on this side of the lazy boundary — so the editor chunk is not
 * downloaded merely to decide what to call a file — and it stays a pure
 * function that vitest can test without a DOM, the same shape as `diff.ts` and
 * `kbTree.ts`.
 *
 * The ids are Monaco's own. Anything not here falls back to `plaintext`, which
 * is the honest answer: no colour beats wrong colour.
 */

/** Files people edit that have no extension, or whose name *is* the type. */
const BY_NAME: Record<string, string> = {
  dockerfile: "dockerfile",
  containerfile: "dockerfile",
  makefile: "makefile",
  gnumakefile: "makefile",
  ".gitignore": "plaintext",
  ".gitattributes": "plaintext",
  ".dockerignore": "plaintext",
  ".env": "shell",
  ".bashrc": "shell",
  ".zshrc": "shell",
  "cargo.lock": "ini",
  "go.mod": "plaintext",
  "go.sum": "plaintext",
};

const BY_EXTENSION: Record<string, string> = {
  // Systems
  rs: "rust",
  go: "go",
  c: "c",
  h: "c",
  cc: "cpp",
  cpp: "cpp",
  cxx: "cpp",
  hpp: "cpp",
  cs: "csharp",
  swift: "swift",
  kt: "kotlin",
  kts: "kotlin",
  java: "java",
  scala: "scala",
  m: "objective-c",
  mm: "objective-c",
  zig: "plaintext",

  // Web
  ts: "typescript",
  tsx: "typescript",
  mts: "typescript",
  cts: "typescript",
  js: "javascript",
  jsx: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  html: "html",
  htm: "html",
  css: "css",
  scss: "scss",
  less: "less",
  vue: "html",
  svelte: "html",

  // Scripting
  py: "python",
  rb: "ruby",
  php: "php",
  pl: "perl",
  lua: "lua",
  r: "r",
  jl: "julia",
  ex: "elixir",
  exs: "elixir",
  clj: "clojure",
  cljs: "clojure",
  dart: "dart",
  sh: "shell",
  bash: "shell",
  zsh: "shell",
  fish: "shell",
  ps1: "powershell",

  // Data and config
  json: "json",
  jsonc: "json",
  yaml: "yaml",
  yml: "yaml",
  toml: "ini",
  ini: "ini",
  cfg: "ini",
  conf: "ini",
  xml: "xml",
  svg: "xml",
  csv: "plaintext",
  sql: "sql",
  graphql: "graphql",
  gql: "graphql",
  proto: "protobuf",

  // Infrastructure
  tf: "hcl",
  tfvars: "hcl",
  hcl: "hcl",
  sol: "sol",

  // Prose
  md: "markdown",
  markdown: "markdown",
  mdx: "markdown",
  txt: "plaintext",
  log: "plaintext",
};

/** Monaco's language id for a project-relative path. */
export function languageFor(path: string): string {
  const name = path.split("/").pop()?.toLowerCase() ?? "";

  const byName = BY_NAME[name];
  if (byName) return byName;

  // `Dockerfile.dev`, `.env.local`, `docker-compose.prod.yml` — the useful part
  // is the leading name or the trailing extension, so try both ends. A leading
  // dot is part of the name rather than a separator, or `.env.local` splits to
  // an empty first segment and matches nothing.
  const leading = name.startsWith(".")
    ? "." + name.slice(1).split(".")[0]
    : name.split(".")[0];
  if (leading && BY_NAME[leading]) return BY_NAME[leading];

  const dot = name.lastIndexOf(".");
  // A name that is *only* an extension (".env") has its dot at 0 and was
  // already handled above; anything else with no dot has no extension.
  if (dot <= 0) return "plaintext";

  return BY_EXTENSION[name.slice(dot + 1)] ?? "plaintext";
}
