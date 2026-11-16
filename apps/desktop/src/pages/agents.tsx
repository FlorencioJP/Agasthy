import type { ActionRequest, AgentSession, ChildRole, Leftover, SessionProcess, SessionUsage, WorktreeView } from "@agasthy/shared-types";
import { Badge, Button, StateView, cn } from "@agasthy/ui";
import { useQuery } from "@tanstack/react-query";
import { Bot, ChevronRight, Cpu, FolderGit2, FolderOpen, GitBranch, MemoryStick, Plug, Server, Square, TriangleAlert } from "lucide-react";
import { useState } from "react";
import { ActionDialog } from "../components/action-dialog";
import { matchSession, totalTokens } from "../components/usage-panel";
import { PageHeader, Panel } from "../components/page";
import { formatBytes, formatDuration, formatRelative, formatTokens, formatUsd, shortenHome } from "../lib/format";
import { api, errorMessage, revealInFileManager } from "../lib/ipc";

const ROLE_LABEL: Record<ChildRole, string> = {
  agent: "sub-agent",
  mcp_server: "MCP server",
  dev_server: "dev server",
  watcher: "watcher",
  shell: "shell",
  keep_awake: "keep-awake",
  other: "process",
};

/** Per-agent accent, so sessions are recognisable at a glance. */
const AGENT_TONE: Record<string, string> = {
  "claude-code": "bg-[#d97757]/15 text-[#c4623f] dark:text-[#e08a6b]",
  codex: "bg-emerald-500/15 text-emerald-700 dark:text-emerald-300",
  "gemini-cli": "bg-sky-500/15 text-sky-700 dark:text-sky-300",
  "cursor-agent": "bg-zinc-500/15 text-fg",
  "app:Cursor": "bg-zinc-500/15 text-fg",
  hermes: "bg-violet-500/15 text-violet-700 dark:text-violet-300",
  openclaw: "bg-rose-500/15 text-rose-700 dark:text-rose-300",
  aider: "bg-lime-500/15 text-lime-700 dark:text-lime-300",
  "copilot-cli": "bg-indigo-500/15 text-indigo-700 dark:text-indigo-300",
};

const stoppable = (p: SessionProcess) => !p.protected;

export function AgentsPage() {
  const report = useQuery({ queryKey: ["agents"], queryFn: api.agentReport, refetchInterval: 5000 });
  const worktrees = useQuery({ queryKey: ["agent-worktrees"], queryFn: api.agentWorktrees, staleTime: 60_000 });
  const usage = useQuery({ queryKey: ["agent-usage"], queryFn: () => api.agentUsage(30), refetchInterval: 30_000 });
  const [action, setAction] = useState<ActionRequest | null>(null);
  const r = report.data;

  if (report.isLoading) return <Shell><StateView kind="loading" title="Looking at running processes" /></Shell>;
  if (report.isError || !r) return <Shell><StateView kind="error" title="Could not read processes" description={errorMessage(report.error)} /></Shell>;

  const maxMem = Math.max(1, ...r.sessions.map((s) => s.total_memory_bytes));
  const mcp = r.sessions.reduce((n, s) => n + s.mcp_servers, 0);
  const servers = r.sessions.reduce((n, s) => n + s.dev_servers, 0);
  const ports = r.sessions.reduce((n, s) => n + s.ports.length, 0);

  return (
    <Shell subtitle={`${r.sessions.length} session${r.sessions.length === 1 ? "" : "s"} · watching since ${formatRelative(r.tracking_since)}`}>
      <div className="flex flex-col gap-4 p-5">
        <div className="grid grid-cols-2 gap-3 md:grid-cols-5">
          <Metric icon={<Bot aria-hidden />} label="Agent sessions" value={String(r.sessions.length)} />
          <Metric icon={<MemoryStick aria-hidden />} label="Memory, all sessions" value={formatBytes(r.total_memory_bytes)} />
          <Metric icon={<Plug aria-hidden />} label="MCP servers" value={String(mcp)} />
          <Metric icon={<Server aria-hidden />} label="Dev servers · ports" value={`${servers} · ${ports}`} hint={r.port_data_available ? undefined : "port data unavailable"} />
          <Metric
            icon={<TriangleAlert aria-hidden />}
            label="Left running"
            value={String(r.leftovers.length)}
            tone={r.leftovers.length > 0 ? "warning" : undefined}
          />
        </div>

        {r.leftovers.length > 0 ? <Leftovers items={r.leftovers} onStop={setAction} /> : null}


        {r.sessions.length === 0 ? (
          <Panel>
            <StateView
              kind="empty"
              title="No coding agents are running"
              description="Start Claude Code, Codex, Gemini CLI, Cursor Agent, Aider or another supported agent in a terminal and it appears here with everything it starts."
            />
          </Panel>
        ) : (
          <div className="grid grid-cols-1 gap-3 lg:grid-cols-2">
            {r.sessions.map((s) => (
              <SessionCard key={s.id} s={s} share={s.total_memory_bytes / maxMem} onAction={setAction} usage={matchSession(usage.data, s.agent, s.cwd, s.root.start_time)} />
            ))}
          </div>
        )}

        <WorktreesPanel loading={worktrees.isLoading} error={worktrees.isError ? errorMessage(worktrees.error) : null} items={worktrees.data?.worktrees ?? []} checked={worktrees.data?.repos_checked ?? 0} onRefresh={() => void worktrees.refetch()} />
      </div>
      {action ? <ActionDialog request={action} onClose={(changed) => { setAction(null); if (changed) void report.refetch(); }} /> : null}
    </Shell>
  );
}

function Shell({ children, subtitle }: { children: React.ReactNode; subtitle?: string }) {
  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader title="Agents" subtitle={subtitle ?? "coding-agent sessions and what they leave behind"} />
      <div className="min-h-0 flex-1 overflow-y-auto">{children}</div>
    </div>
  );
}

function Metric({ icon, label, value, hint, tone }: { icon: React.ReactNode; label: string; value: string; hint?: string; tone?: "warning" }) {
  return (
    <div className={cn("rounded-lg border bg-surface px-3.5 py-3", tone === "warning" ? "border-warning/40" : "border-line")}>
      <div className={cn("flex items-center gap-1.5 text-[11.5px] [&_svg]:size-3.5", tone === "warning" ? "text-warning" : "text-muted")}>
        {icon}
        {label}
      </div>
      <div className="tabular mt-1 text-[20px] font-semibold leading-7 text-fg">{value}</div>
      {hint ? <div className="text-[11px] text-faint">{hint}</div> : null}
    </div>
  );
}

function Leftovers({ items, onStop }: { items: Leftover[]; onStop: (a: ActionRequest) => void }) {
  const targets = items.filter((l) => stoppable(l.process)).map((l) => ({ pid: l.process.pid, start_time: l.process.start_time }));
  const mem = items.reduce((n, l) => n + l.process.memory_bytes, 0);
  return (
    <section className="rounded-lg border border-warning/40 bg-warning/[0.06]" aria-label="Left running">
      <div className="flex items-center justify-between gap-3 border-b border-warning/30 px-3.5 py-2.5">
        <div className="min-w-0">
          <div className="text-[13px] font-medium text-fg">
            {items.length} process{items.length === 1 ? "" : "es"} still running after {items.length === 1 ? "its" : "their"} agent exited · {formatBytes(mem)}
          </div>
          <div className="text-[11.5px] text-muted">Dev servers, MCP servers and watchers keep memory and ports busy until stopped.</div>
        </div>
        <Button size="sm" variant="primary" disabled={targets.length === 0} onClick={() => onStop({ type: "terminate_processes", targets })}>
          <Square aria-hidden /> Review stopping {targets.length}
        </Button>
      </div>
      <ul className="divide-y divide-line/60">
        {items.map((l) => (
          <li key={`${l.process.pid}-${l.process.start_time}`} className="flex items-center gap-3 px-3.5 py-2 text-[12.5px]">
            <Badge tone="warning">{ROLE_LABEL[l.process.role]}</Badge>
            <div className="min-w-0 flex-1">
              <div className="truncate text-fg">
                {l.process.name} <span className="text-faint">PID {l.process.pid}</span>
                {l.process.ports.length ? <span className="ml-1.5 text-muted">:{l.process.ports.map((p) => p.port).join(", :")}</span> : null}
              </div>
              <div className="truncate text-[11.5px] text-muted">
                {l.origin === "seen_in_session" && l.agent_label
                  ? `Started by ${l.agent_label}${l.session_cwd ? ` in ${shortenHome(l.session_cwd)}` : ""}${l.agent_exited_at ? `, which exited ${formatRelative(l.agent_exited_at)}` : ""}`
                  : `Parent exited; who started it is unknown${l.project ? ` · ${l.project.name}` : ""}`}
              </div>
            </div>
            <span className="tabular text-muted">{formatBytes(l.process.memory_bytes)}</span>
          </li>
        ))}
      </ul>
    </section>
  );
}

function SessionCard({ s, share, onAction, usage }: { s: AgentSession; share: number; onAction: (a: ActionRequest) => void; usage: SessionUsage | null }) {
  const [open, setOpen] = useState(false);
  const servers = s.children.filter((c) => (c.role === "dev_server" || c.role === "watcher") && stoppable(c));
  const where = s.project?.name ?? (s.cwd ? shortenHome(s.cwd) : "unknown folder");
  return (
    <article className="flex flex-col rounded-lg border border-line bg-surface" aria-label={`${s.label} session`}>
      <header className="flex items-start gap-3 p-3.5">
        <span className={cn("flex size-8 shrink-0 items-center justify-center rounded-md text-[12px] font-semibold", AGENT_TONE[s.agent] ?? "bg-accent/15 text-accent")}>
          {s.label.slice(0, 2)}
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="text-[13px] font-semibold text-fg">{s.label}</span>
            <span className="truncate text-[12.5px] text-muted">{where}</span>
          </div>
          <div className="mt-0.5 flex flex-wrap items-center gap-x-2.5 gap-y-1 text-[11.5px] text-faint">
            <span>PID {s.root.pid}</span>
            <span>up {formatDuration(s.root.run_time_secs)}</span>
            {s.host ? <span>in {s.host}</span> : null}
            {s.recognised ? null : <span title="Found because it starts MCP servers">detected via MCP</span>}
          </div>
          {usage ? (
            <div className="tabular mt-1 text-[11.5px] text-muted" title={usage.models.map((m) => m.model).join(", ")}>
              {formatTokens(totalTokens(usage.tokens))} tokens · {formatTokens(usage.tokens.output)} out · {usage.tokens.requests.toLocaleString()} requests
              {usage.estimated_cost_usd != null ? ` · ≈${formatUsd(usage.estimated_cost_usd)}${usage.cost_partial ? "+" : ""}` : ""}
            </div>
          ) : null}
        </div>
        <div className="text-right">
          <div className="tabular text-[15px] font-semibold text-fg">{formatBytes(s.total_memory_bytes)}</div>
          <div className="tabular text-[11.5px] text-muted">{s.total_cpu_percent == null ? "CPU —" : `${s.total_cpu_percent.toFixed(0)}% CPU`}</div>
        </div>
      </header>
      <div className="px-3.5">
        <div className="h-1 overflow-hidden rounded-full bg-raised" role="meter" aria-label="Share of agent memory" aria-valuenow={Math.round(share * 100)} aria-valuemin={0} aria-valuemax={100}>
          <div className="h-full rounded-full bg-accent/70" style={{ width: `${Math.max(3, share * 100)}%` }} />
        </div>
      </div>
      <div className="flex flex-wrap items-center gap-1.5 px-3.5 pt-2.5 text-[11.5px]">
        {s.mcp_servers ? <Badge tone="info"><Plug className="size-3" aria-hidden /> {s.mcp_servers} MCP</Badge> : null}
        {s.dev_servers ? <Badge tone="accent"><Server className="size-3" aria-hidden /> {s.dev_servers} dev server{s.dev_servers === 1 ? "" : "s"}</Badge> : null}
        {s.nested_agents ? <Badge><Bot className="size-3" aria-hidden /> {s.nested_agents} sub-agent{s.nested_agents === 1 ? "" : "s"}</Badge> : null}
        {s.ports.map((p) => (
          <Badge key={`${p.address}:${p.port}`} tone="neutral">:{p.port}</Badge>
        ))}
      </div>
      <div className="mt-auto flex items-center justify-between gap-2 px-3.5 py-2.5">
        <button onClick={() => setOpen((o) => !o)} aria-expanded={open} className="flex items-center gap-1 text-[12px] text-muted hover:text-fg">
          <ChevronRight className={cn("size-3.5 transition-transform", open && "rotate-90")} aria-hidden />
          {s.children.length} process{s.children.length === 1 ? "" : "es"} started
        </button>
        <div className="flex items-center gap-1.5">
          {s.cwd ? (
            <Button size="sm" variant="ghost" aria-label="Show folder" onClick={() => void revealInFileManager(s.cwd!)}>
              <FolderOpen aria-hidden />
            </Button>
          ) : null}
          {servers.length ? (
            <Button size="sm" onClick={() => onAction({ type: "terminate_processes", targets: servers.map((c) => ({ pid: c.pid, start_time: c.start_time })) })}>
              Stop its servers
            </Button>
          ) : null}
          {stoppable(s.root) ? (
            <Button size="sm" variant="ghost" onClick={() => onAction({ type: "terminate_process", pid: s.root.pid, start_time: s.root.start_time, force: false })}>
              End session
            </Button>
          ) : null}
        </div>
      </div>
      {open ? (
        <ul className="border-t border-line text-[12px]">
          {s.children.map((c) => (
            <li key={c.pid} className="flex items-center gap-2.5 border-b border-line/50 px-3.5 py-1.5 last:border-0">
              <span className="w-[84px] shrink-0 text-[11px] text-muted">{ROLE_LABEL[c.role]}</span>
              <span className="min-w-0 flex-1 truncate font-mono text-[11.5px] text-fg" title={c.command}>
                {c.command}
              </span>
              {c.ports.length ? <span className="text-[11px] text-muted">:{c.ports.map((p) => p.port).join(", :")}</span> : null}
              <span className="tabular w-16 text-right text-muted">{formatBytes(c.memory_bytes)}</span>
            </li>
          ))}
        </ul>
      ) : null}
    </article>
  );
}

function WorktreesPanel({ items, loading, error, checked, onRefresh }: { items: WorktreeView[]; loading: boolean; error: string | null; checked: number; onRefresh: () => void }) {
  return (
    <Panel
      title={
        <span className="flex items-center gap-1.5">
          <FolderGit2 className="size-3.5" aria-hidden /> Git worktrees in scanned repositories
        </span>
      }
      actions={
        <Button size="sm" variant="ghost" onClick={onRefresh}>
          Refresh
        </Button>
      }
      bodyClassName="p-0"
    >
      {loading ? (
        <StateView kind="loading" title="Reading worktrees" compact />
      ) : error ? (
        <StateView kind="error" title={error} compact />
      ) : items.length === 0 ? (
        <div className="p-3.5 text-[12.5px] text-muted">
          No extra worktrees in {checked} scanned repositor{checked === 1 ? "y" : "ies"}. Agents that work in parallel (Claude Code, Codex, Conductor) create them; they appear here with their changes.
        </div>
      ) : (
        <table className="w-full text-[12.5px]">
          <thead className="text-left text-[11px] text-muted">
            <tr className="border-b border-line">
              <th className="px-3.5 py-2 font-medium">Worktree</th>
              <th className="px-2 py-2 font-medium">Branch</th>
              <th className="px-2 py-2 font-medium">Created by</th>
              <th className="px-2 py-2 font-medium">Changes</th>
              <th className="px-3.5 py-2 text-right font-medium">Modified</th>
            </tr>
          </thead>
          <tbody>
            {items.map((w) => (
              <tr key={w.path} className="border-b border-line/50 last:border-0">
                <td className="max-w-0 px-3.5 py-2">
                  <div className="truncate text-fg" title={w.path}>
                    {shortenHome(w.path)}
                  </div>
                  <div className="text-[11px] text-faint">{w.repo_name}</div>
                </td>
                <td className="px-2 py-2 text-muted">
                  <span className="inline-flex items-center gap-1">
                    <GitBranch className="size-3" aria-hidden />
                    {w.branch ?? `detached ${w.head ?? ""}`}
                  </span>
                </td>
                <td className="px-2 py-2">{w.created_by ? <Badge>{w.created_by}</Badge> : <span className="text-faint">—</span>}</td>
                <td className="px-2 py-2">
                  {w.prunable ? (
                    <Badge tone="warning">folder missing</Badge>
                  ) : w.active_agents.length ? (
                    <Badge tone="success">
                      <Cpu className="size-3" aria-hidden /> {w.active_agents.join(", ")} working
                    </Badge>
                  ) : w.changed_files == null ? (
                    <span className="text-faint">unknown</span>
                  ) : w.changed_files > 0 ? (
                    <Badge tone="warning">{w.changed_files} uncommitted</Badge>
                  ) : (
                    <span className="text-muted">clean</span>
                  )}
                </td>
                <td className="tabular px-3.5 py-2 text-right text-muted">{w.modified_at ? formatRelative(w.modified_at) : "—"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </Panel>
  );
}
