<p align="center">
  <img src="apps/desktop/app-icon.svg" width="88" alt="AGASTHY mark" />
</p>

<h1 align="center">AGASTHY</h1>

<p align="center"><strong>Your developer machine. One intelligence layer.</strong><br/>
Discover · Understand · Connect · Optimize · Operate</p>

---

AGASTHY is an open-source, local-first desktop application that discovers what is on a developer's machine, works out how the pieces relate, and explains it with evidence.

> **Status: early development (v0.1.0, Phase 4).** It runs on macOS. It has not been published or released, and nothing here is offered as a download yet.

## Why it exists

A developer machine accumulates hundreds of components spread across directories, terminals and configuration files: cloned repositories, applications, CLI tools, several copies of the same runtime, packages from several package managers, and more. Answering simple questions, such as "which Python actually runs?", "what did Homebrew install?" or "which of my clones have uncommitted work?", means running a dozen commands.

AGASTHY reads the machine directly and keeps what it finds, plus the evidence behind each fact, in a local database.

## What works today (macOS)

| Area | What AGASTHY does |
|---|---|
| **Live resources** | CPU (total and per core), memory, swap, disk per volume, load average, process count, top processes by memory and CPU, GPU model. Anything it cannot measure is shown as *unavailable* with a reason, never as 0. |
| **Repositories** | In folders **you choose**: finds Git working trees and reports branch, HEAD commit, last commit time, working-tree status (staged / modified / untracked / conflicted), local branch count, remotes (credentials stripped), GitHub origin, languages, frameworks, licence, manifests, size on disk, and duplicate clones of the same project. |
| **Applications** | Bundles in `/Applications`, `~/Applications` and `/System/Applications`, read from `Info.plist`. |
| **CLI tools & runtimes** | A curated catalogue of about 75 developer tools. For each copy on PATH: version, which copy is active and which are shadowed, and how it was installed (for example, it resolves into a Homebrew keg). |
| **Packages** | Read-only inventories from Homebrew (formulae and casks), npm and pnpm globals, uv tools, pip (default `python3`) and `cargo install`. A missing or failing manager is reported as such, never as "0 packages". |
| **Provenance** | Every fact carries evidence and a confidence level (*verified*, *strong*, *probable*, *unknown*). A Git remote read from the repository's own config is *verified*; an upstream taken from a Homebrew formula's homepage is *strong*. Nothing is guessed from names. |
| **Dependencies** | Reads `package.json`, `Cargo.toml`, `pyproject.toml`, `requirements.txt` and `go.mod` (including nested packages), with resolved versions from `package-lock.json`, `Cargo.lock` and `uv.lock`. Shows which dependencies your projects share. |
| **Runtime requirements & impact** | Links projects to the runtimes they declare (`engines.node`, `.nvmrc`, `requires-python`, `rust-version`, `go`, …). Answers *"what breaks if I remove this runtime?"*: for each project, whether it would break, fall back to another copy on PATH, or be unaffected, and what is unknown. Nothing is removed; it is a simulation. |
| **Machine Graph** | An interactive graph of projects, runtimes, dependencies, packages, binaries and upstream projects, with focus, depth and kind filters, search, and the evidence behind every edge. |
| **AI ecosystem** | Finds AI coding assistants, desktop AI apps and local inference engines from evidence on disk; the providers they use (with how strongly each is confirmed); MCP servers in every known config; local models; and AI instruction files (`CLAUDE.md`, `AGENTS.md`, Cursor/Copilot rules, agents, skills, commands) globally and per project, with duplicates, overlaps, orphans and potential conflicts. Credential files are only checked for existence and secrets are redacted. |
| **Processes** | Every process with CPU, memory, listening ports, the project it runs in, and why it is protected if it is. Stop or force-quit your own processes after a reviewed confirmation; system and other users' processes are refused. |
| **Storage & cleanup** | Measures dependency folders and build output in your projects (only those Git ignores and that contain no tracked files) and a fixed list of developer caches. You pick items, review a dry run, and they move to a quarantine you can restore from; space is freed when you delete them permanently. Docker build cache and dangling images can be pruned; volumes never are. |
| **Audit & privacy** | An append-only audit log of every action, and a privacy page listing exactly which programs AGASTHY may run. |

Repositories, installed packages, binaries on PATH and applications are kept as **separate kinds**. A local clone is never treated as installed software.

### Not built yet

Planned, but **not** in this build: security advisories, impact analysis, the update lab, snapshots and the AI operator. See [ROADMAP.md](ROADMAP.md). The app's sidebar shows only the sections that work.

## Screenshots

No screenshots are included yet. Screenshots of a real machine show its hostname, applications and processes, so they will be added from a neutral demo machine.

## Supported platforms

| Platform | Status |
|---|---|
| macOS 12+ (Apple Silicon) | Implemented and tested on macOS 15 / M2. |
| macOS (Intel) | Expected to work; not yet tested. |
| Windows 11 | Architecture in place; adapters are a skeleton that report "unsupported". Not built or tested on Windows yet; CI is configured to check it. |
| Linux | Skeleton adapter; same status as Windows. |

## Architecture

A Rust workspace with a thin Tauri 2 shell and a React UI:

```
apps/desktop/            React + TypeScript UI; src-tauri/ is a thin command layer
crates/core/             domain model, SQLite store (FTS5), migrations, audit log
crates/platform-adapters/ OS adapters (macOS / Windows / Linux) and the guarded command runner
crates/discovery/        read-only discovery: repositories, software, packages
crates/provenance/       remote-URL parsing, credential redaction, confidence rules
crates/graph/            graph views, shared dependencies, runtime impact analysis
crates/process-manager/  processes, listening ports, guarded termination
crates/cleanup/          disk analysis, cleanup safety checks, quarantine, Docker usage
crates/actions/          typed action engine: plan (dry run) → confirm → execute → verify → audit
crates/system-monitor/   CPU / memory / disk / process telemetry
crates/cli/              `agasthy` headless CLI (JSON output)
packages/shared-types/   TypeScript types generated from the Rust model (ts-rs)
packages/ui/             shared React components
migrations/              SQL schema
```

See [docs/architecture.md](docs/architecture.md) for the data model, safety boundaries and how to add a platform.

## Development setup

Requirements: Rust (stable, 1.90+), Node.js 20+, pnpm 10, and on macOS the Xcode Command Line Tools.

```sh
pnpm install
pnpm dev                 # runs the desktop app (Vite + Tauri)
```

Checks:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm typecheck && pnpm lint && pnpm test
pnpm e2e                 # Playwright UI tests (uses installed Chrome locally)
pnpm check:types         # generated TS types match the Rust model
```

Headless discovery (prints JSON, changes nothing):

```sh
cargo run -p agasthy-cli -- scan ~/code --depth 4
cargo run -p agasthy-cli -- software
cargo run -p agasthy-cli -- packages
cargo run -p agasthy-cli -- telemetry
cargo run -p agasthy-cli -- impact runtime:/usr/local/bin/python3 --db <file>
```

Build an unsigned macOS app bundle:

```sh
pnpm build               # output: target/release/bundle/macos/AGASTHY.app
```

Details: [docs/development.md](docs/development.md).

## Privacy

AGASTHY makes no internet requests in this build: no account, telemetry or update checks (the AI scan only checks whether local inference engines are listening on 127.0.0.1). It only looks for repositories in folders you add, stores results in a local SQLite file, and strips credentials from Git URLs before storing them. It runs external programs only from an allowlist, with fixed arguments and never through a shell. Full details: [docs/privacy.md](docs/privacy.md).

## Roadmap

Development is phased: foundation → MVP discovery → intelligence graph → AI ecosystem → system operations and safe cleanup → impact analysis, snapshots and security → AI operator and full Windows/Linux support. The live checklist is in [ROADMAP.md](ROADMAP.md).

## Contributing

Contributions are welcome. Please read [CONTRIBUTING.md](CONTRIBUTING.md) and [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). Report security issues as described in [SECURITY.md](SECURITY.md), not in public issues.

## Name

*AGASTHY* is the product name. A suggested backronym, not a historical meaning: **A**utonomous **G**overnance, **A**nalysis, **S**ystems, **T**ooling & **H**ybrid intelligence.

## License

Apache License 2.0. See [LICENSE](LICENSE).
