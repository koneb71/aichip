/**
 * The one Node API the tests use, declared rather than pulling in all of
 * @types/node for a browser app: tests that check a file as written (the
 * stylesheet's tokens, the source-scanning design test) read it from disk.
 */
declare module "node:fs" {
  export function readFileSync(path: URL | string, encoding: "utf8"): string;
  export function readdirSync(path: URL | string, options: { recursive: true }): string[];
}
