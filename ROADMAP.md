# Roadmap

A box is ticked only when the item is implemented **and** verified. Anything partial is described as such.

## Phase 0: Repository foundation

- [x] Monorepo: Cargo workspace + pnpm workspace
- [x] Tauri 2 + React 19 + TypeScript + Vite + Tailwind CSS desktop shell
- [x] Shared types generated from Rust (ts-rs) with a staleness check (`pnpm check:types`)
- [x] SQLite with embedded, versioned migrations; FTS5 full-text index
- [x] Platform adapter interfaces (`SystemPaths`, `AppInventory`, `ProbePolicy`, `Platform`)
- [x] Guarded command runner (allowlist, no shell, timeouts, output caps) + mock for tests
- [x] Append-only audit log (enforced by database triggers)
- [x] Original icon and wordmark; light and dark themes
- [x] CI workflows (macOS, Windows, Linux) and security workflow, *written but not yet run on GitHub*
- [x] Architecture, privacy and development documentation
- [x] Desktop shell builds and launches on macOS

## Phase 1: Functional MVP (macOS)

- [x] User-selected scan folders with validation (no system roots, no symlink escape, canonical paths)
- [x] Git repository discovery: branch, HEAD, last commit, working tree, branches, remotes, size
- [x] Remote parsing and GitHub origin detection; credential redaction
- [x] Duplicate clone detection
- [x] Language, framework, manifest and licence detection
- [x] Installed applications (macOS `Info.plist`)
- [x] Catalogued CLI tools and runtimes on PATH with versions, active vs shadowed copies, install hints
- [x] Package inventories: Homebrew, npm, pnpm, uv, pip, cargo
- [x] Binary → package links (Homebrew keg, npm global, uv tool, pnpm global)
- [x] CPU, memory, swap, disk, load, process and GPU-model telemetry
- [x] SQLite persistence with evidence and confidence, pruning of items no longer present
- [x] Overview dashboard with real data, empty/loading/error/permission states
- [x] Discovery UI: tabs, full-text search, filters, sorting, detail panel with evidence and relationships
- [x] Settings: folders, scans, scan history with issues, audit log; Privacy page
- [x] Headless CLI for verification
- [ ] Windows: adapter skeleton exists and is wired into CI; **not yet compiled or run on Windows**
- [ ] Yarn global inventory (reported as "not supported yet")
- [x] Unsigned release `.app` built and run on macOS 15 / Apple Silicon (scan → store → display verified)
- [ ] `.dmg` packaging: needs the terminal to have Automation permission for Finder; bundle target is `app` until then
- [ ] Release build verified on a second machine; signing and notarisation

## Phase 2: Intelligence graph

- [x] `graph` crate over the existing entity/relationship tables (schema v2: relationship attributes)
- [x] Manifest parsing: `package.json`, `Cargo.toml`, `pyproject.toml` (PEP 621, Poetry, dependency groups), `requirements.txt`, `go.mod`; nested packages in monorepos
- [x] Resolved versions from lockfiles: `package-lock.json`, `Cargo.lock`, `uv.lock`
- [x] Runtime requirements: `engines.node`, `.nvmrc`, `.node-version`, `requires-python`, `.python-version`, `rust-version`, `rust-toolchain(.toml)`, `go` directive, `.tool-versions`, Gemfile `ruby`; implied use marked *probable*
- [x] Version-constraint evaluation (npm, Cargo, PEP 440, version files); unevaluable constraints reported as unknown
- [x] Runtime impact analysis: "what breaks if I remove this runtime copy?" with PATH-order fallback, evidence and stated limitations
- [x] Most-shared dependencies
- [x] Machine Graph view (React Flow + dagre): Projects and Installed-software overviews, focus mode with depth and kind filters, node search, evidence and impact in the side panel
- [ ] `pnpm-lock.yaml` and `poetry.lock` resolved versions
- [ ] Impact analysis for packages and dependencies (not only runtimes)
- [ ] Graph layout for very large graphs (clustering)

## Phase 3: AI ecosystem

- [x] AI tool discovery (21 tools) by executable, app bundle (bundle id preferred over name), editor extension and config directory; config-only tools are not claimed as installed
- [x] Provider evidence levels: referenced → configured → endpoint configured → credential present → reachable (local only); credential files checked for existence only, env vars by name only
- [x] Local inference engines with a 127.0.0.1 port check; local models from Ollama, LM Studio and the Hugging Face cache
- [x] AI files: instructions, rules, agents, skills, commands, settings and MCP configs, global and per project, with scope (global / project / directory)
- [x] Redacted, searchable instruction text; duplicates (SHA-256), overlaps, orphaned files, `@import` references
- [x] Potential conflicts for explicit settings (package manager, indentation, quotes) between files that apply together, with line evidence
- [x] MCP servers from Claude Code, Claude Desktop, Codex, Gemini CLI, Cursor, VS Code and Windsurf configs, with redacted args and env var names only
- [x] AI Ecosystem page with question shortcuts; AI view in the Machine Graph
- [ ] Windows and Linux locations for the same tools
- [ ] JetBrains AI Assistant and other IDE plugins
- [x] Natural-language questions (Phase 6, AI Operator)

## Phase 4: System operations

- [x] Typed action engine: dry-run plan → single-use, expiring confirmation → re-validation → execution → verification → audit (refused attempts audited too)
- [x] Process manager: CPU, memory, ports, project link, classification; graceful stop and separate force quit; PID + start-time targeting; system, other-user, self and parent processes refused
- [x] Listening TCP ports via `lsof` (current user's processes)
- [x] Disk analysis: node_modules, Rust target, Python venvs, build output (Git-ignored, no tracked files), package and developer caches, model caches, duplicate clones (review only)
- [x] Same-volume quarantine with restore and explicit permanent delete
- [x] Docker usage; prune build cache or dangling images on confirmation; volumes never touched
- [ ] macOS Trash integration ("Put Back") as an alternative to AGASTHY's quarantine
- [ ] Cleanup across volumes
- [ ] Restarting services (launchd, brew services) and opening a terminal at a process location
- [x] Windows port listing (`netstat -ano`; unvalidated on hardware)
- [ ] Windows process ownership

## Phase 5: Advanced intelligence

- [x] Change impact simulator for packages, runtimes, dependencies, tools and apps (Homebrew dependents from installed kegs' runtime dependencies, transitive; runtimes provided by dependents; projects affected), with risk, unknowns, plan and recovery
- [x] Safe Update Lab: Git worktree per lab; manifest edit without reformatting; install (scripts off unless allowed) and test as separately approved actions; apply only manifest + lockfile when the project is unchanged; discard
- [x] Environment snapshots: capture, list, compare (two snapshots or with now), reconstruction commands for missing items (not run by AGASTHY)
- [x] Security and lifecycle: outdated Homebrew packages, unknown-provenance tools, opt-in OSV.dev advisories with a preview of exactly what is sent
- [ ] OSV coverage for more ecosystems and transitive dependencies from lockfiles
- [ ] Archived / unmaintained upstream detection (needs GitHub API, opt-in)
- [ ] Update Lab for pip/uv and Yarn; build step; side-by-side test comparison with the original

## Phase 6: AI operator and platforms

- [x] AI Operator: built-in deterministic mode (no model, nothing sent) for common questions
- [x] Provider-agnostic model mode: OpenAI-compatible endpoints (Ollama, LM Studio, vLLM, LocalAI, OpenAI, Gemini, OpenRouter, custom) and Anthropic; tool-calling loop over 12 typed tools
- [x] The model can only read through typed tools and propose actions; proposals go through the existing dry-run → confirm → verify → audit flow; it cannot execute anything
- [x] Cloud models need a separate opt-in; API keys in the OS credential store; home paths shown as `~`; instruction-file text never sent; every question audited with where its data went
- [x] Windows adapter: application inventory from registry Uninstall keys (`reg query`), Scoop and Chocolatey 2.x inventory, listening ports; unit-tested against recorded output
- [x] Linux adapter: `.desktop` application inventory (including Flatpak and Snap exports), Flatpak and Snap package inventory; unit-tested
- [ ] Validate the Windows and Linux adapters on real hardware (status stays "unvalidated" until then)
- [ ] WinGet inventory (its `list` output is a localized table), Windows services, GPU information on Windows and Linux
- [ ] Streaming Operator answers; conversation history beyond the current session

## Agent Control Center

- [x] Agents page: sessions for 14 recognised coding agents plus any process that starts MCP servers; process trees, memory, CPU, ports, MCP/dev servers, host app
- [x] Processes left running after an agent exits, attributed to their session; reviewed batch stop
- [x] Git worktrees with creator, uncommitted changes and active agents
- [x] Token Usage page from Claude Code and Codex logs and Hermes Agent's database (counters only); `agasthy usage`
- [ ] Windows: graceful stop (no SIGTERM; needs a WM_CLOSE / console-control path) and detached-process detection (Windows does not re-parent orphans to PID 1)
- [ ] Usage readers for more agents as their local formats are verified (OpenClaw, Gemini CLI, OpenCode)
- [ ] Worktree cleanup action (remove a clean, inactive worktree through `git worktree remove`)
