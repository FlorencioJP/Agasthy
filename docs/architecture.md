# Architecture

## Principles

1. **Real data only.** Every value shown comes from the machine. A value that could not be measured is an explicit `Availability::Unavailable | PermissionDenied | Unsupported` with a reason, never a zero or a placeholder.
2. **Evidence for every claim.** Entities and relationships carry evidence records (what was observed, where, with what confidence).
3. **Read-only discovery, separate mutation.** Discovery crates only observe. Every change goes through the typed action engine (`crates/actions`).
4. **Platform logic behind traits.** Shared crates never branch on the operating system.
5. **Thin shell.** `apps/desktop/src-tauri` holds state and command wiring only.

## Crates

```
agasthy-core ─────────────┐  domain model, Store (SQLite), audit log
agasthy-platform ─────────┤  OS adapters + CommandRunner
agasthy-provenance ───────┤  remote parsing, redaction, confidence rules
agasthy-system-monitor ───┤  telemetry (sysinfo)
agasthy-discovery ────────┤  repositories, manifests, software, packages → IngestBatch
agasthy-graph ────────────┤  views, shared dependencies, impact analysis (reads the Store)
agasthy-actions ──────────┤  typed action engine: plan → confirm → re-validate → verify → audit
agasthy-ai ───────────────┘  AI Operator: tools, built-in router, provider loop (OpenAI-compatible, Anthropic)
        │
        ├── agasthy-cli       headless JSON output
        └── agasthy-desktop   Tauri commands → React UI
```

Dependencies point one way. `core` depends on nothing internal, and `discovery` never touches the database: it returns an `IngestBatch`, which the caller writes with `Store::ingest`.

## Data model

Stored in SQLite (`migrations/0001_init.sql`):

| Table | Purpose |
|---|---|
| `entities` | One row per discovered thing. `kind` ∈ repository, remote_project, application, binary, runtime, package, package_manager. Kind-specific data lives in `attributes` (JSON), whose shape is a typed Rust struct exported to TypeScript. |
| `relationships` | Typed edges: `origin_remote`, `tracks_remote`, `managed_by`, `upstream_project`, `provides`, `installed_by`, `duplicate_clone`, `depends_on`, `requires_runtime`, each with a confidence level and kind-specific `attributes` (for example the declarations behind a `depends_on` edge: manifest, constraint, scope, resolved version). |
| `evidence` | Observations backing an entity *or* a relationship. |
| `scans` | One row per scan with status, counts and non-fatal issues. |
| `scan_roots` | Folders the user approved for repository discovery. |
| `audit_log` | Append-only (UPDATE/DELETE blocked by triggers). |
| `entities_fts` | FTS5 index over name, path and attributes, kept in sync by triggers. |

Entity IDs are deterministic (`repository:<path>`, `package:homebrew:<name>`, `remote:github.com/<owner>/<name>`), so rescans upsert rather than duplicate.

**Ingest semantics.** A scan declares the scopes it is authoritative for (`PruneScope { kind, source }`). After upserting, rows in scope that were not seen are removed from AGASTHY's database (never from the machine). If a package manager fails, its scope is not declared, so its previous results are kept. Relationships are pruned only when their source entity was re-observed without them. Relationships whose endpoint does not exist yet are skipped rather than invented.

**Confidence.**

| Level | Example |
|---|---|
| verified | A repository's own Git config names the remote; a binary resolves into a Homebrew keg. |
| strong | A Homebrew formula's homepage names a GitHub project; an npm global path. |
| probable | Indirect inference (reserved). |
| unknown | Name similarity only; never used to create a relationship. |

## Graph and impact analysis

The graph is the `entities` and `relationships` tables; there is no separate graph store. `agasthy-graph` provides:

- **Views.** `neighborhood` does a breadth-first walk from a node (depth 1–3, kind filter, node cap; package managers are not expanded through because they link to every package). `overview` shows either *Projects* (repositories, required runtimes, dependencies shared by 2+ projects, upstreams linked twice or more) or *Installed software*.
- **Runtime impact.** For a runtime copy, each project with a `requires_runtime` edge is classified as *breaks*, *falls back*, *unaffected* (the copy is shadowed), *already incompatible* or *unknown*. The successor is the next copy in AGASTHY's PATH order, and constraints are evaluated by `version_req` (npm, Cargo, PEP 440, version files). Constraints such as `lts/*` or `stable`, and projects that only imply a runtime, are *unknown*, never guessed. Each report states its limitations (version managers, other shells' PATH, non-project consumers).

`depends_on` edges come from manifests (*verified*: the file states it). `requires_runtime` edges are *verified* when a version is declared and *probable* when use is only implied by a manifest's presence. Dependencies are declarations, not installations; the UI says so.

## AI ecosystem discovery

`agasthy-discovery::ai` builds one `IngestBatch` (scan kind `ai`):

- **Tools**: a catalogue (`ai/catalog.rs`) of assistants, desktop apps and local engines, each detected from executables, application bundles (bundle identifier preferred over file name), VS Code-style extension folders and config paths. A tool with only configuration is recorded with `installed: false`.
- **Providers**: evidence levels from built-in vendor ties, tool settings (names and keys only), environment variable *names*, and credential files' *existence*. Local engines get a `127.0.0.1` TCP check.
- **Files**: fixed global locations plus a walk of each scanned repository that only enters known AI directories. Instruction-type text is redacted (`ai/redact.rs`) and capped; configuration files are parsed but not stored.
- **Analysis**: SHA-256 duplicates, overlaps (several instruction files applying to one directory), orphaned files (no reader installed), `@import` references, and potential conflicts. Conflicts are only reported when two files that apply together each state exactly one value for the same setting, with no negation on the line.

## Actions (changes to the machine)

`agasthy-actions` is the only code path that changes anything outside AGASTHY's own data:

1. `plan(ActionRequest)` performs a dry run: it resolves exact targets, sizes, warnings, reversibility and recovery, and runs every safety check. If it can run, it returns a single-use token (10-minute expiry).
2. The UI shows the plan; the user confirms.
3. `execute(token)` re-validates every target (the disk may have changed), performs the action, verifies the result (process gone; original path gone and quarantine copy present), and writes an audit entry. Unknown, reused or expired tokens are refused and audited.

Action types: `TerminateProcess { pid, start_time, force }`, `QuarantinePaths`, `RestoreQuarantine`, `PurgeQuarantine`, `DockerPrune { BuildCache | DanglingImages }`. Cleanup paths must come from the latest disk analysis and pass `cleanup::safety::check_path` (absolute, not a symlink, canonical, not a repository or `.git`, not containing a known repository, inside an approved folder or a listed cache root) and, for project artifacts, `check_untracked` (Git-ignored, no tracked files).

## Safety boundaries

**Command runner** (`crates/platform-adapters/src/command.rs`):
- Programs must be on an allowlist, matched by file name (also for absolute paths).
- Shells, `env`, `sudo`, `osascript` and similar are refused even if listed.
- Arguments are an array; nothing is interpreted by a shell.
- stdin is closed, every call has a timeout, and stdout/stderr are capped.

**Git.** Every invocation passes `-c core.fsmonitor=false -c core.hooksPath=/dev/null -c diff.external=` and sets `GIT_CONFIG_NOSYSTEM=1`, so repository-local configuration cannot make Git run other programs during a status read.

**Version probes.** Tools are run with update checks, telemetry and download prompts disabled (`PROBE_ENV`). On macOS, `/usr/bin` stubs that would open an installer dialog (Xcode CLT, Java) are not run.

**Filesystem walking.** Scan roots are canonicalised once and checked against protected locations. The walker never follows symlinks, skips dependency and build directories, and is bounded by depth, directory count and file count. Untrusted files (`package.json`, `LICENSE`, …) are read only if they are regular files under a size cap.

**Secrets.** Credentials in remote URLs (`https://user:token@…`, query strings) are removed before storage or display.

## AI Operator

`crates/ai-engine` defines the Operator's tools (`TOOLS`: JSON schemas), the `ToolHost` trait, a deterministic built-in router, and the provider loop. The desktop app implements `ToolHost` (`apps/desktop/src-tauri/src/operator.rs`) over the Store, process monitor, disk analysis and Docker usage, shortening home paths to `~` and dropping instruction-file text. `propose_action` builds an `ActionRequest` and dry-runs it with the action engine to describe it; the UI opens proposals in `ActionDialog`, which plans again and needs confirmation. The model never sees a confirmation token.

## Adding a platform

Implement `SystemPaths`, `AppInventory`, `ProbePolicy` and `Platform` in `crates/platform-adapters/src/<os>/`, and select it in `current()`. The Windows and Linux modules compile on every OS so their parsers are tested on any machine; keep OS-specific system calls behind commands run through the guarded runner. Return `Availability::Unsupported` for anything not yet implemented, so the UI says so instead of showing an empty list.

## Frontend

- React 19, TanStack Router (hash history) and TanStack Query, with Tailwind CSS v4 tokens on `:root`, plus a dark theme.
- `src/lib/ipc.ts` is the only place that calls Tauri. Tests can replace it with a fixture (`window.__AGASTHY_TEST_IPC__`) that is never set in the app.
- Outside the desktop shell (a plain browser), the UI shows an explanatory state instead of sample data.
