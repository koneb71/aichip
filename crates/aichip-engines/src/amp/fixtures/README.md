# Amp fixtures

`task.jsonl`'s lines are Amp's own documented example
(https://ampcode.com/docs/markdown/cli/streaming-json, read on 2026-10-02),
with the elided values filled in and the tool's input made concrete.
`error.jsonl` is built from the error `result` shape on the same page, whose
`error` is a string rather than Claude Code's `result`.

Neither has been recorded from a real `amp` binary. Replace them with a run
(`amp -x "…" --stream-json --dangerously-allow-all > task.jsonl`) the first
time someone has the CLI to hand.
