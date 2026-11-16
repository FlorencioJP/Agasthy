# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

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
