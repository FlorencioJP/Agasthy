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
- **No network.** v0.1.0 makes no network requests of its own.
- **Read-only.** v0.1.0 has no actions that modify the machine. Later mutation features will be typed, approval-gated, dry-run-first and audited.
- **Webview capabilities.** The Tauri capability file grants only AGASTHY's own commands, the folder picker and reveal-in-file-manager.

Reports that any of these boundaries can be bypassed are especially welcome.
