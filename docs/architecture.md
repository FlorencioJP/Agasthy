# Architecture

## Principles

1. **Real data only.** Every value shown comes from the machine. A value that could not be measured is an explicit `Availability::Unavailable | PermissionDenied | Unsupported` with a reason, never a zero or a placeholder.
2. **Evidence for every claim.** Entities and relationships carry evidence records (what was observed, where, with what confidence).
3. **Read-only discovery, separate mutation.** Discovery crates only observe. Later phases add mutation through a separate, typed, approval-gated layer.
4. **Platform logic behind traits.** Shared crates never branch on the operating system.
5. **Thin shell.** `apps/desktop/src-tauri` holds state and command wiring only.

## Crates

```
agasthy-core ─────────────┐  domain model, Store (SQLite), audit log
agasthy-platform ─────────┤  OS adapters + CommandRunner
agasthy-provenance ───────┤  remote parsing, redaction, confidence rules
agasthy-system-monitor ───┤  telemetry (sysinfo)
agasthy-discovery ────────┘  repositories, software, packages → IngestBatch
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
| `relationships` | Typed edges: `origin_remote`, `tracks_remote`, `managed_by`, `upstream_project`, `provides`, `installed_by`, `duplicate_clone`, each with a confidence level. |
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

## Adding a platform

Implement `SystemPaths`, `AppInventory`, `ProbePolicy` and `Platform` in `crates/platform-adapters/src/<os>/`, and select it in `current()`. Return `Availability::Unsupported` for anything not yet implemented, so the UI says so instead of showing an empty list.

## Frontend

- React 19, TanStack Router (hash history) and TanStack Query, with Tailwind CSS v4 tokens on `:root`, plus a dark theme.
- `src/lib/ipc.ts` is the only place that calls Tauri. Tests can replace it with a fixture (`window.__AGASTHY_TEST_IPC__`) that is never set in the app.
- Outside the desktop shell (a plain browser), the UI shows an explanatory state instead of sample data.
