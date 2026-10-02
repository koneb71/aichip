## What changed and why

<!-- What this does, and the reason it is wanted. Link the issue if there is one. -->

## How it was tested

<!-- Tests added or run (`cargo test`, `pnpm test`, one test by name), and anything checked by hand in the dashboard — in both themes for a screen change. -->

## Checklist

- [ ] Tests added or updated (inline `mod tests` / `mod db_tests`, or vitest under `web/src`), with no model usage — the mock engine, fixtures or a stand-in binary.
- [ ] Docs updated where affected: `README.md`, `docs/architecture.md`, `CLAUDE.md`, `SECURITY.md`, `.env.example` (`crates/eren-cli/src/docs_tests.rs` checks engines, capabilities, `EREN_*` settings and migrations).
- [ ] Compliance invariants respected: official binaries from `PATH` only, spawned through `env_guard::command`; no credentials read, stored or forwarded; no auth variables set on a child; no engine traffic proxied.
- [ ] Web changes use the design tokens and the `web/src/components/ui/` kit, and the design scan passes.
- [ ] No AI attribution in commits, this description, or code comments.
