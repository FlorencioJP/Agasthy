import { StateView } from "@agasthy/ui";
import type { ReactNode } from "react";
import { PageHeader, Panel } from "../components/page";
import { shortenHome } from "../lib/format";
import { errorMessage } from "../lib/ipc";
import { useAppInfo } from "../lib/queries";

function Item({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="flex flex-col gap-1">
      <div className="text-[12.5px] font-medium text-fg">{title}</div>
      <div className="text-[12.5px] leading-5 text-muted">{children}</div>
    </div>
  );
}

export function PrivacyPage() {
  const info = useAppInfo();
  return (
    <>
      <PageHeader title="Privacy" subtitle="What AGASTHY reads, stores and transmits in this build" />
      <div className="flex max-w-4xl flex-col gap-4 p-5">
        <Panel title="Transmits">
          <div className="flex flex-col gap-3">
            <Item title="AGASTHY makes no internet requests in this build.">
              No account, no telemetry, no analytics, no update checks. Nothing about your machine, your code or your configuration is uploaded.
            </Item>
            <Item title="Local ports only, for local AI engines.">
              When an installed local inference engine is found (Ollama, LM Studio, Jan, vLLM, LocalAI), the AI scan opens a TCP connection to its port on{" "}
              <code className="font-mono">127.0.0.1</code> to see whether it is running, then closes it. No data is sent. Cloud providers are never contacted.
            </Item>
            <Item title="Tools it runs may use their own caches.">
              AGASTHY runs package managers to list what they installed. It sets opt-outs such as <code className="font-mono">HOMEBREW_NO_AUTO_UPDATE</code>,{" "}
              <code className="font-mono">NO_UPDATE_NOTIFIER</code> and <code className="font-mono">COREPACK_ENABLE_NETWORK=0</code>, but a tool may still refresh
              its own metadata cache according to its own settings.
            </Item>
          </div>
        </Panel>
        <Panel title="Reads">
          <div className="flex flex-col gap-3">
            <Item title="Project folders you choose">
              Inside each folder: the presence of <code className="font-mono">.git</code>, Git metadata via the <code className="font-mono">git</code> command, the
              names of manifest files, at most the first part of <code className="font-mono">LICENSE</code>, <code className="font-mono">package.json</code>,{" "}
              <code className="font-mono">Cargo.toml</code>, <code className="font-mono">pyproject.toml</code> and similar to detect licences and frameworks, and file
              sizes and extensions. Source code contents are not stored.
            </Item>
            <Item title="Applications">
              The <code className="font-mono">Info.plist</code> of bundles in /Applications, ~/Applications and /System/Applications (name, identifier, version).
            </Item>
            <Item title="Command-line tools">
              Whether a catalogued tool exists in the directories below, and the output of its version command.
            </Item>
            <Item title="AI configuration">
              Fixed locations in your home folder (for example <code className="font-mono">~/.claude</code>, <code className="font-mono">~/.codex</code>,{" "}
              <code className="font-mono">~/.cursor</code>, <code className="font-mono">~/.gemini</code>, Claude Desktop's config), editor extension folder
              names, local model directories, and AI instruction files inside repositories you scanned. Credential files (such as{" "}
              <code className="font-mono">~/.codex/auth.json</code>) are checked for existence only and never opened. Of environment variables, only the names
              are checked.
            </Item>
            <Item title="System resources">CPU, memory, swap, disk and process statistics from the operating system, sampled while the app is open.</Item>
          </div>
        </Panel>
        <Panel title="Stores">
          <div className="flex flex-col gap-3">
            <Item title="A local SQLite database">
              {info.data ? <code className="selectable font-mono text-[12px] text-fg">{info.data.db_path}</code> : "In the application data folder."} It holds scan
              results, the evidence behind them, your folder list and an append-only audit log. Delete the file to remove everything.
            </Item>
            <Item title="AI instruction text, redacted">
              Up to 16 KB of each instruction, rules, agent, skill or command file is stored for search and preview, after credential-like values are
              redacted. Configuration files (settings, MCP configs) are parsed for server names and commands; their contents are not stored, and
              environment values passed to MCP servers are never stored, only their names.
            </Item>
            <Item title="Secrets are not stored">
              Credentials embedded in Git remote URLs (for example <code className="font-mono">https://user:token@host/…</code>) are removed before anything is
              saved or displayed. Telemetry samples are kept in memory only.
            </Item>
          </div>
        </Panel>
        <Panel title="Runs">
          {info.isLoading ? (
            <StateView kind="loading" title="Loading" compact />
          ) : info.error ? (
            <StateView kind="error" title="Could not read build information" description={errorMessage(info.error)} compact />
          ) : info.data ? (
            <div className="flex flex-col gap-3">
              <Item title="Only these programs, with fixed arguments, never through a shell">
                <div className="mt-1 flex flex-wrap gap-1" role="list" aria-label="Command allowlist">
                  {info.data.command_allowlist.map((p) => (
                    <code key={p} role="listitem" className="rounded bg-raised px-1.5 py-px font-mono text-[11.5px] text-fg">
                      {p}
                    </code>
                  ))}
                </div>
                <div className="mt-2">
                  Shells, <code className="font-mono">sudo</code> and similar programs are refused even if listed. Git runs with repository-configured helper programs
                  (such as <code className="font-mono">core.fsmonitor</code>) disabled. AGASTHY changes nothing on your machine in this build: there are no delete,
                  install, update or terminate actions.
                </div>
              </Item>
              <Item title="Directories searched for tools, in order">
                <ul className="mt-1 flex flex-col gap-0.5">
                  {info.data.search_dirs.map((d) => (
                    <li key={d.path} className="flex gap-2 font-mono text-[11.5px]">
                      <span className="selectable text-fg">{shortenHome(d.path)}</span>
                      <span className="text-faint">
                        {d.origin === "process_path" ? "PATH" : d.origin === "system_path_config" ? "/etc/paths" : "conventional location"}
                      </span>
                    </li>
                  ))}
                </ul>
              </Item>
            </div>
          ) : null}
        </Panel>
      </div>
    </>
  );
}
