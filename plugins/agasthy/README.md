# AGASTHY plugin for Claude Code and Codex

One install gives your agent:

- the **AGASTHY skill**: when to ask AGASTHY and how to read its answers;
- the **AGASTHY MCP server** (`agasthy mcp`): nine read-only tools for repositories, packages, impact analysis, coding agents, processes and ports, token usage and system load.

Everything is read-only. Changes stay in the AGASTHY app, behind a dry-run review.

## Install

First put the `agasthy` command-line tool on your PATH (see [integrations/mcp](../../integrations/mcp/README.md#1-install-the-agasthy-tool)) and install the [AGASTHY app](https://github.com/HMAKT99/Agasthy/releases/latest) to scan your machine.

**Claude Code**

```text
/plugin marketplace add HMAKT99/Agasthy
/plugin install agasthy@agasthy
```

**Codex**

```bash
codex plugin marketplace add HMAKT99/Agasthy
codex plugin add agasthy@agasthy
```

The skill here is a copy of [`skills/agasthy/SKILL.md`](../../skills/agasthy/SKILL.md); CI keeps them identical.
