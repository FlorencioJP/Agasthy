# Security Policy

## Supported versions

AGASTHY is pre-release (0.x). Only the latest commit on `main` receives fixes.

## Reporting a vulnerability

Please **do not** open a public issue for security problems. Once the project's public repository exists, use GitHub's private vulnerability reporting ("Report a vulnerability" under the repository's *Security* tab). Until then, contact the maintainers privately.

Please include affected versions, reproduction steps and the impact you observed. We aim to acknowledge reports within a few days.

## Security model

AGASTHY runs with the user's privileges and inspects their machine, so the boundaries it keeps are part of its design:

- **No shell execution.** External programs run through `CommandRunner`: an allowlist of program names, fixed argument arrays, closed stdin, timeouts and output caps. Shells, `env`, `sudo` and similar are always refused.
- **No privilege escalation.** AGASTHY never invokes `sudo` or requests elevated rights.
- **Git hardening.** Git runs with `core.fsmonitor`, hooks and external diff disabled, and system config ignored, so a repository's configuration cannot make Git run other programs during discovery.
- **Untrusted input.** Repository files, command output and plist contents are treated as data: size-capped, parsed, never executed.
- **Filesystem bounds.** Scan roots are canonicalised and checked against protected locations; walks never follow symlinks and are bounded.
- **Secrets.** Credentials embedded in Git remote URLs are stripped before storage or display.
- **No internet access.** v0.1.0 makes no internet requests. The AI scan only opens TCP connections to `127.0.0.1` on local inference engines' ports to see whether they are running.
- **AI configuration.** Credential files are checked for existence only and never opened; environment variables are checked by name only; instruction text is redacted before storage; AI instruction files are treated as untrusted data and never executed or followed.
- **Changes only through the action engine.** Every change is a typed `ActionRequest` (`crates/actions`): planned as a dry run, confirmed with a single-use token that expires, re-validated immediately before running, verified afterwards and audited (refused attempts included). There is no generic "run a command" action.
- **Cleanup boundaries.** Only regenerable artifacts in approved folders (Git-ignored, containing no tracked files) or a fixed list of cache locations; never a repository, a `.git` directory, a symlink, the home folder, or anything outside those roots. Items are moved into a same-volume quarantine and can be restored.
- **Process boundaries.** Processes are addressed by PID and start time. System processes, other users' processes, AGASTHY and its parent are refused. Force quit is a separate request.
- **Webview capabilities.** The Tauri capability file grants only AGASTHY's own commands, the folder picker and reveal-in-file-manager.

Reports that any of these boundaries can be bypassed are especially welcome.
