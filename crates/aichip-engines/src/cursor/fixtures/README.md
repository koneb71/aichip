# Cursor CLI fixtures

**Synthetic.** Built by hand on 2026-10-02 from
https://cursor.com/docs/cli/reference/output-format.md, because no
`cursor-agent` binary was available where the adapter was written.

Two parts are guesses and are marked as such in the parser:

- the `function` tool-call shape's `result` (the docs show only `readToolCall`
  and `writeToolCall` results), and
- the names of the `usage` fields on `result`, which the changelog promises
  but the reference does not spell out.

Replace `task.jsonl` with a recorded run (`cursor-agent -p --output-format
stream-json --force --trust "…" > task.jsonl`) the first time someone has the
CLI to hand.
