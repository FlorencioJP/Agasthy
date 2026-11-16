## What and why

<!-- What does this change, and why is it needed? Link the issue. -->

## How it was verified

<!-- Commands run, platforms tested, screenshots of the real app if UI changed. -->

## Checklist

- [ ] `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass
- [ ] `pnpm typecheck`, `pnpm lint`, `pnpm test` and `pnpm e2e` pass
- [ ] `pnpm check:types` passes (bindings regenerated if Rust types changed)
- [ ] No sample or fabricated data in the app; missing values use `Availability`
- [ ] New claims carry evidence and an honest confidence level
- [ ] No new external program without adding it to the allowlist deliberately
- [ ] Anything that modifies the machine is approval-gated, dry-run-first and audited
- [ ] `ROADMAP.md` / `CHANGELOG.md` updated if behaviour changed
