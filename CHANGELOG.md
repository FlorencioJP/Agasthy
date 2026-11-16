# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added (Phase 6)
- AI Operator page: a built-in mode that answers common questions with no model, and an optional model mode with a tool-calling loop over typed, read-only tools.
- Providers: Ollama, LM Studio, vLLM, LocalAI, OpenAI, Google Gemini, OpenRouter and custom OpenAI-compatible endpoints, and Anthropic (Messages API). Keys stored in the OS credential store; cloud models need an explicit opt-in.
- Proposals from the Operator (stop a process, quarantine cleanup candidates, prune Docker) open the existing dry-run review; the Operator cannot execute anything.
- Settings → AI model panel; operator questions recorded in the audit log with where data was sent.
- Windows adapter: registry application inventory, Scoop and Chocolatey 2.x packages, listening ports via `netstat`. Linux adapter: `.desktop` applications, Flatpak and Snap packages. Both unit-tested; reported as "unvalidated" until run on real hardware.

### Added (Phase 5)
- Impact Analysis: removal simulation for any entity, Homebrew dependents from installed runtime dependencies, runtime and project impact.
- Safe Update Lab: Git worktree labs with approved install/test steps and guarded apply.
- Snapshots: capture, compare, reconstruction plans.
- Security: outdated Homebrew packages, unknown-origin tools, opt-in OSV.dev advisories.
- CLI: `agasthy impact <id>` now covers any entity; `agasthy security` (local checks).

### Added (Phase 4)
- Typed action engine (`agasthy-actions`) with dry-run plans, single-use confirmations, re-validation, verification and audit.
- Processes screen: live process table with ports, project links and protection reasons; stop and force quit with review.
- Storage & Cleanup screen: disk analysis, Git-verified project artifacts and developer caches, restorable quarantine, Docker usage and prune.
- CLI: `agasthy processes`, `agasthy disk` (read-only).

### Added (Phase 3)
- AI ecosystem discovery: assistants, desktop AI apps and local inference engines with on-disk evidence; providers with evidence levels; local models; MCP servers; AI instruction, agent, skill, command, settings and MCP files.
- Duplicate, overlap, orphan, `@import` and potential-conflict analysis for AI instruction files.
- Secret redaction for stored instruction text and MCP arguments; credential files checked for existence only.
- AI Ecosystem screen with question shortcuts; AI view in the Machine Graph; `agasthy ai` CLI command.

### Added (Phase 2)
- Project dependency discovery from manifests and lockfiles across npm, Cargo, PyPI and Go, including nested packages in monorepos.
- Runtime requirement discovery and `requires_runtime` links to the runtime copies on PATH.
- `agasthy-graph` crate: neighbourhood and overview graph views, most-shared dependencies, runtime impact analysis with a version-constraint evaluator.
- Machine Graph screen; a Project dependencies tab in Discovery; impact analysis in runtime details; "Scan everything" runs scans in dependency order.
- Schema v2 migration: relationships carry attributes (declarations, constraints, resolved versions).
- CLI: `agasthy impact <runtime-id> --db FILE`, `agasthy shared --db FILE`.

### Added (Phase 1)
- Monorepo foundation: Rust workspace, Tauri 2 desktop shell, React 19 UI, shared TypeScript types generated from Rust.
- SQLite store with migrations, FTS5 search, evidence and confidence for every entity and relationship, and an append-only audit log.
- macOS platform adapter; Windows and Linux adapter skeletons.
- Guarded command runner (allowlist, no shell, timeouts, output caps).
- Git repository discovery in user-chosen folders: branch, HEAD, working tree, remotes (credentials redacted), GitHub origin, duplicate clones, languages, frameworks, licence, size.
- Application discovery (macOS bundles), catalogued CLI tools and runtimes with versions and active/shadowed resolution, install-source hints.
- Package inventories: Homebrew, npm, pnpm, uv, pip, cargo; links between binaries, packages, applications and upstream projects.
- Live telemetry: CPU, memory, swap, disks, load, processes, GPU model.
- Overview, Discovery, Settings and Privacy screens.
- `agasthy` headless CLI.
- CI and security workflows (not yet run on GitHub).
