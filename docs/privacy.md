# Privacy

This describes AGASTHY v0.1.0. The in-app **Privacy** page shows the live values for your machine (database path, command allowlist, searched directories).

## Transmits

- **Nothing.** AGASTHY makes no network requests: no account, telemetry, analytics, crash reporting or update checks.
- Package managers that AGASTHY runs to list installed packages may refresh their *own* metadata caches according to their own settings. AGASTHY sets opt-outs where they exist (`HOMEBREW_NO_AUTO_UPDATE`, `HOMEBREW_NO_ANALYTICS`, `NO_UPDATE_NOTIFIER`, `COREPACK_ENABLE_NETWORK=0`, `DOTNET_CLI_TELEMETRY_OPTOUT`, `CHECKPOINT_DISABLE`, …).

## Reads

| What | Where | Notes |
|---|---|---|
| Git metadata | Folders you add in Settings | Via `git` (branch, HEAD, status counts, remotes, branch list). |
| Project files | Same folders | Names of manifest files, sizes and extensions of files; the start of `LICENSE`, and `package.json` / `Cargo.toml` / `pyproject.toml` / `requirements.txt` / `go.mod` / `Gemfile` to detect a licence or framework (size-capped). Source code contents are not stored. |
| Applications | `/Applications`, `~/Applications`, `/System/Applications` | `Contents/Info.plist` only. |
| CLI tools | Your `PATH`, `/etc/paths`, `/etc/paths.d`, conventional install folders | Existence of catalogued tool names, and the output of their version command. |
| Packages | Package managers | The output of one read-only listing command per manager. |
| System resources | The operating system | CPU, memory, swap, disks, processes (name, PID, memory, CPU, executable path). Kept in memory only. |

AGASTHY never scans your whole disk. Repository discovery only looks inside folders you add, and system locations and `~/Library` cannot be added.

## Stores

A single SQLite database:

- macOS: `~/Library/Application Support/org.agasthy.desktop/agasthy.db`

It contains scan results with their evidence, your folder list, scan history and the audit log. Credentials embedded in Git remote URLs are stripped before storage. Delete the file to remove everything AGASTHY has stored.

## Runs

Only allowlisted programs, with fixed arguments, never through a shell: `git`, `brew`, `npm`, `pnpm`, `uv`, `python3` (for `pip list`), `cargo`, `system_profiler`, and the catalogued developer tools, which are run only with their version argument. Shells and privilege-escalation tools are always refused. AGASTHY never uses `sudo`.

AGASTHY v0.1.0 does not modify your machine: there are no delete, install, update or terminate actions.
