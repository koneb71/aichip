# Gemini CLI fixtures

**Synthetic.** Built by hand on 2026-10-02 from gemini-cli's own schema
(`packages/core/src/output/types.ts`, `stream-json-formatter.ts` at commit
`fb972b2`), because no `gemini` binary was available where the adapter was
written. The docs publish no example lines.

Replace them with a recorded run (`gemini --prompt=… --output-format=stream-json
--approval-mode=auto_edit --skip-trust > task.jsonl`) the first time someone
has the CLI to hand, and fix whatever the parser got wrong.

- `task.jsonl` — a run that reads, fails one edit, writes, and finishes.
  The stats include thinking tokens (`total_tokens` minus prompt minus output).
- `quota.jsonl` — a run that ends on `TerminalQuotaError`.
