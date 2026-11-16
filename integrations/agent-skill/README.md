# AGASTHY skill for coding agents

Give your agent real facts about your machine. With this skill, OpenClaw, Hermes, Claude Code and other agents that read [Agent Skills](https://agentskills.io) can answer:

- "Which of my projects have uncommitted work?"
- "What is using port 3000?"
- "How many tokens did my agents use this week, and on which project?"
- "Which coding agents left dev servers running?"
- "Is it safe to remove openssl@3?"

The agent runs `agasthy`, AGASTHY's command-line tool, and answers from its JSON output. The tool is **read-only**: it reads what the AGASTHY app has already scanned (opening its database read-only) plus live process and token-usage data. It never changes your machine. The skill also tells the agent to leave any change to you, through the AGASTHY app's dry-run review.

## Install

The skill is one file: [`agasthy/SKILL.md`](agasthy/SKILL.md).

**OpenClaw**

```bash
mkdir -p ~/.openclaw/skills/agasthy
curl -fsSL https://raw.githubusercontent.com/HMAKT99/Agasthy/main/integrations/agent-skill/agasthy/SKILL.md -o ~/.openclaw/skills/agasthy/SKILL.md
```

OpenClaw then offers to download the `agasthy` tool for your system (macOS Apple silicon, Linux x64, Windows x64).

**Hermes**

```bash
hermes skills install HMAKT99/Agasthy/integrations/agent-skill/agasthy
```

**Claude Code**

```bash
mkdir -p ~/.claude/skills/agasthy
curl -fsSL https://raw.githubusercontent.com/HMAKT99/Agasthy/main/integrations/agent-skill/agasthy/SKILL.md -o ~/.claude/skills/agasthy/SKILL.md
```

For Hermes, Claude Code and others, install the `agasthy` tool yourself: download `agasthy-cli-<system>` from the [latest release](https://github.com/HMAKT99/Agasthy/releases/latest), check it against the `.sha256` file next to it, unpack it, and put `agasthy` on your PATH. On macOS:

```bash
cd /tmp && curl -fsSLO https://github.com/HMAKT99/Agasthy/releases/latest/download/agasthy-cli-macos-aarch64.tar.gz \
  && curl -fsSLO https://github.com/HMAKT99/Agasthy/releases/latest/download/agasthy-cli-macos-aarch64.tar.gz.sha256 \
  && shasum -a 256 -c agasthy-cli-macos-aarch64.tar.gz.sha256 \
  && mkdir -p ~/.local/bin && tar -C ~/.local/bin -xzf agasthy-cli-macos-aarch64.tar.gz \
  && ~/.local/bin/agasthy --version
```

## The command-line tool

```text
agasthy status                       when each kind of data was last scanned
agasthy find [text] [--kind KIND]    search repositories, packages, runtimes, tools, MCP servers...
agasthy entity <id>                  one item with its evidence and relationships
agasthy impact <id>                  what breaks if it were removed
agasthy agents                       live coding-agent sessions and what they left running
agasthy processes                    processes, ports and the project each belongs to
agasthy usage [--days N]             token usage and estimated cost per agent
agasthy --help                       everything else
```

Query commands read the AGASTHY app's database, so install the [app](https://github.com/HMAKT99/Agasthy/releases/latest) and run a scan first. `agents`, `processes`, `usage` and `telemetry` work without it.
