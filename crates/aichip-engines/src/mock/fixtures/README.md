# Mock stream fixtures

Hand-written `claude --output-format stream-json` transcripts replayed by the
mock engine. Unlike `opencode/fixtures/`, these are **synthesised, not
recorded** — the mock engine exists to drive aichip's own paths with zero model
usage, so a fixture here is a test input, not evidence about a CLI's behaviour.
Anything asserting what a real CLI emits belongs in a recorded fixture instead.

| file | shows |
|---|---|
| `simple_task.jsonl` | the happy path: init, text, two tool calls, a `result` with cost and usage |
| `cached_task.jsonl` | a run that reads mostly from cache — `cache_read_input_tokens` far exceeding `input_tokens` |
| `interrupted_task.jsonl` | usage reported, then the stream **stops**: no `result` line, ever |

`interrupted_task.jsonl` is the one worth explaining. It reproduces the shape of
a cancelled run or a dead engine, where the only token figures that will ever
exist are the per-message ones. Before the usage tally, a run ending this way
recorded **zero** tokens — a twenty-minute session you interrupted showed as
free, and the daily budget under-counted it to match. Replaying this fixture
drives `stream_run` down the "event stream ended unexpectedly" path, which is
the regression that fixture guards.

Note the input counts in it climb (300 → 640 → 980). That is deliberate:
Claude Code reports each *request's* usage, so summing those numbers would
claim 1920 tokens for a run that used 980. See `aichip-core`'s `usage_tally`
for the rule that prevents it.
