# Privacy

This describes AGASTHY v0.1.0. The in-app **Privacy** page shows the live values for your machine (database path, command allowlist, searched directories).

## Transmits

- **No account, telemetry, analytics or crash reporting.** Cloud AI providers are contacted only if you configure one *and* turn on cloud models (below).
- **Update check (on by default, can be turned off in Settings → About).** About eight seconds after start-up, AGASTHY downloads `latest.json` from this project's GitHub Releases (`github.com/HMAKT99/Agasthy`) to see whether a newer version exists. The request carries nothing about your machine beyond what any HTTPS request reveals (your IP address, to GitHub). An update is downloaded only when you click **Install and restart**, and is installed only if its signature matches the public key built into the app.
- **AI Operator.** The built-in Operator uses no model and sends nothing. If you configure a model in Settings → AI model:
  - A **local** endpoint (`127.0.0.1` / `localhost`, e.g. Ollama or LM Studio) receives your question and the Operator's tool results on this machine.
  - A **cloud** endpoint requires a separate switch, *Allow sending machine data to a cloud model*, which is off by default. With it on, the endpoint receives your question, the conversation so far in this session, and tool results: process names, PIDs and memory/CPU figures, item names and versions, paths with your home folder shown as `~`, sizes, counts, MCP server names and transports, finding titles. It never receives AI instruction-file text, environment variable values, API keys or file contents.
  - Requests go through `curl` with the request body and headers in owner-only temporary files that are deleted afterwards, so keys never appear on a command line. Every question is written to the audit log with the provider, the number of tool calls and where the data went.
- **Optional: OSV.dev advisory checks (off by default).** If you turn this on (Security page), each security check sends package *name, ecosystem and version* triples to `https://api.osv.dev` with `curl`, and fetches advisory details by ID. No paths, project names, hostnames or credentials are sent. The Security page shows exactly what would be sent before you enable it, and each check is written to the audit log with the number of versions sent.
- **Update Lab steps you approve** run `npm`, `pnpm` or `cargo` in the lab copy; installs download packages from those registries, and test runs execute the project's own test command.
- **Local ports, for local AI engines only.** When an installed local inference engine is found (Ollama 11434, LM Studio 1234, Jan 1337, vLLM 8000, LocalAI 8080), the AI scan opens and closes a TCP connection to that port on `127.0.0.1` to see if it is running. No data is sent.
- Package managers that AGASTHY runs to list installed packages may refresh their *own* metadata caches according to their own settings. AGASTHY sets opt-outs where they exist (`HOMEBREW_NO_AUTO_UPDATE`, `HOMEBREW_NO_ANALYTICS`, `NO_UPDATE_NOTIFIER`, `COREPACK_ENABLE_NETWORK=0`, `DOTNET_CLI_TELEMETRY_OPTOUT`, `CHECKPOINT_DISABLE`, …).

## Reads

| What | Where | Notes |
|---|---|---|
| Git metadata | Folders you add in Settings | Via `git` (branch, HEAD, status counts, remotes, branch list). |
| Project files | Same folders | Names of manifest files, sizes and extensions of files; the start of `LICENSE`, and `package.json` / `Cargo.toml` / `pyproject.toml` / `requirements.txt` / `go.mod` / `Gemfile` to detect a licence or framework (size-capped). Source code contents are not stored. |
| Applications | `/Applications`, `~/Applications`, `/System/Applications` | `Contents/Info.plist` only. |
| CLI tools | Your `PATH`, `/etc/paths`, `/etc/paths.d`, conventional install folders | Existence of catalogued tool names, and the output of their version command. |
| Packages | Package managers | The output of one read-only listing command per manager. |
| AI configuration | Fixed home locations (`~/.claude`, `~/.claude.json`, `~/.codex`, `~/.gemini`, `~/.cursor`, `~/.codeium/windsurf`, `~/.continue`, `~/.aider.conf.yml`, Claude Desktop and VS Code MCP configs), editor extension folder names, local model stores (`~/.ollama`, `~/.lmstudio`, `~/.cache/huggingface/hub`), and AI files inside scanned repositories | Credential files (`~/.codex/auth.json`, `~/.gemini/oauth_creds.json`, `~/.cache/huggingface/token`, …) are checked for existence only and never opened. Environment variables: names only. |
| System resources | The operating system | CPU, memory, swap, disks, processes (name, PID, memory, CPU, executable path). Kept in memory only. |

AGASTHY never scans your whole disk. Repository discovery only looks inside folders you add, and system locations and `~/Library` cannot be added.

## Agent sessions and token usage

- **Agents page.** Every 45 seconds while AGASTHY runs, it reads the process table (as the Processes page does) to group coding-agent processes into sessions and to remember which processes each session started, so that it can say what was left running after an agent exited. This list is kept in memory only.
- **Token Usage page.** AGASTHY reads coding agents' own local usage records: Claude Code session logs (`~/.claude/projects/**/*.jsonl`), Codex session logs (`~/.codex/sessions/**/*.jsonl`) and Hermes Agent's database (`~/.hermes/state.db`, opened read-only; only token and cost columns are selected). Only token counters, model names, timestamps and the session folder are kept, in memory; prompts, replies and tool output are never stored, shown or sent. Cost figures are estimates at API list prices.
- Git worktrees of scanned repositories are listed with `git worktree list --porcelain` and `git status`.

## Stores

A diagnostics log (`logs/agasthy.log` in the app data folder, at most about 4 MB) records start-up (version and OS), crashes, failed scans and failed background tasks, with your home folder written as `~`. It is never uploaded; Settings → About shows where it is so you can attach it to a bug report.

API keys for AI providers are stored only in the operating system's credential store (macOS Keychain, Windows Credential Manager, the Linux kernel keyring), under the service `org.agasthy.desktop`. They are never written to AGASTHY's database, logs or files. The chosen provider, endpoint and model are stored in the database.

A single SQLite database:

- macOS: `~/Library/Application Support/org.agasthy.desktop/agasthy.db`

It contains scan results with their evidence, your folder list, scan history and the audit log. Credentials embedded in Git remote URLs are stripped before storage. For AI instruction-type files (instructions, rules, agents, skills, commands) up to 16 KB of text is stored for search and preview after credential-like values are redacted. Settings and MCP configuration files are parsed but their contents are not stored; for MCP servers only the command, redacted arguments, URL without credentials, and the *names* of environment variables are kept. Delete the file to remove everything AGASTHY has stored.

## Runs

Only allowlisted programs, with fixed arguments, never through a shell: `git`, `brew`, `npm`, `pnpm`, `uv`, `python3` (for `pip list`), `cargo`, `system_profiler`, and the catalogued developer tools, which are run only with their version argument. Shells and privilege-escalation tools are always refused. AGASTHY never uses `sudo`.

Also `curl` (OSV.dev when enabled, and the AI model endpoint you configure), `lsof` (to list listening TCP ports) and `docker` (`system df`, `images --filter dangling=true --quiet`, and, only after you confirm, `builder prune --force` or `image prune --force`).

## Changes

Changes happen only when you confirm a reviewed plan. In this build that means: stopping or force-quitting a process you own, or stopping several (for example what an agent left running); moving cleanup candidates into AGASTHY's quarantine; restoring or permanently deleting quarantined items; and pruning Docker build cache or dangling images. Each one is planned as a dry run first (exact targets, sizes, risks, recovery), re-validated just before it runs, verified afterwards and written to the audit log. AGASTHY never uses `sudo`, never deletes Git repositories or files tracked by Git, never removes Docker volumes, and never stops system processes or other users' processes.

Snapshots are JSON files in `~/Library/Application Support/org.agasthy.desktop/snapshots/` containing application, tool, package and repository metadata and environment variable *names* (never values). Update Labs live in `…/labs/` as Git worktrees of your projects.

Quarantine lives in `~/Library/Application Support/org.agasthy.desktop/quarantine/`. Items are moved there with a rename on the same disk (never copied), so disk space is freed only when you choose **Delete permanently**.
