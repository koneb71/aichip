# Recorded OpenCode streams

Golden fixtures for `opencode/stream_parser.rs`. Captured from **opencode 1.18.9**
on 2026-07-29 against `google/gemini-3.6-flash`, with
`opencode run "<prompt>" --format json --auto < /dev/null`.

| file | prompt | shows |
|---|---|---|
| `simple_text.ndjson` | "Reply with exactly: ok" | the minimal stream: `step_start`, `text`, `step_finish` |
| `tool_and_two_steps.ndjson` | "Read the file sample.txt and reply with its second line only." | a `tool_use` (state `completed`, with `input`/`output`/`title`), and **two** steps under two distinct `messageID`s |

`tool_and_two_steps.ndjson` is the one that settles cost accounting: step 2 costs
*less* than step 1 (0.0140955 vs 0.0152055), so a `step_finish` carries its own
message's cost rather than a running total. Summing across distinct `messageID`s
is therefore correct; re-check this if the CLI version moves.

Note what is **absent**: there is no terminal event. The stream simply ends, which
is why `RunCompleted` is synthesised by the adapter on EOF rather than by the parser.
