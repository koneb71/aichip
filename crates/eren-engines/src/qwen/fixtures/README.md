# Qwen Code fixtures

**Synthetic.** Built by hand on 2026-10-02 from qwen-code 0.24.7's stream
types (`packages/cli/src/nonInteractive/types.ts`, the init built in
`nonInteractiveHelpers.ts`), because no `qwen` binary was available where the
adapter was written.

- `task.jsonl` — a run that writes a file and finishes.
- `auth_error.jsonl` — what a run with no provider key configured ends with:
  an error `result` whose reason is in `error.message`, not `result`.

Replace them with recorded runs (`qwen --output-format=stream-json
--approval-mode=auto-edit "…" > task.jsonl`) the first time someone has the
CLI to hand.
