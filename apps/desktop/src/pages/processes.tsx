import type { ActionRequest, ProcessInfo, ProcessKind } from "@agasthy/shared-types";
import { Badge, Button, StateView, cn } from "@agasthy/ui";
import { useQuery } from "@tanstack/react-query";
import { FolderOpen, Lock, Search, Square, X, Zap } from "lucide-react";
import { useState } from "react";
import { ActionDialog } from "../components/action-dialog";
import { Avail } from "../components/availability";
import { Kv, PageHeader } from "../components/page";
import { formatBytes, formatDuration, formatNumber, formatPercent, shortenHome, unavailableReason, value } from "../lib/format";
import { api, errorMessage, revealInFileManager } from "../lib/ipc";

const KIND_LABEL: Record<ProcessKind, string> = {
  system: "System",
  application: "App",
  developer_server: "Dev server",
  database: "Database",
  ai_tool: "AI",
  container: "Container",
  other: "Other",
};

const FILTERS: { id: string; label: string; test: (p: ProcessInfo) => boolean }[] = [
  { id: "mine", label: "Mine (stoppable)", test: (p) => !p.protected },
  { id: "dev", label: "Dev servers & databases", test: (p) => p.kind === "developer_server" || p.kind === "database" },
  { id: "listening", label: "Listening on ports", test: (p) => p.ports.length > 0 },
  { id: "projects", label: "In my projects", test: (p) => p.project != null },
  { id: "ai", label: "AI", test: (p) => p.kind === "ai_tool" },
  { id: "apps", label: "Apps", test: (p) => p.kind === "application" },
  { id: "all", label: "All", test: () => true },
];

type SortKey = "memory" | "cpu" | "name";

export function ProcessesPage() {
  const q = useQuery({ queryKey: ["processes"], queryFn: api.processes, refetchInterval: 3000 });
  const [filter, setFilter] = useState("mine");
  const [text, setText] = useState("");
  const [sort, setSort] = useState<SortKey>("memory");
  const [selected, setSelected] = useState<number | null>(null);
  const [action, setAction] = useState<ActionRequest | null>(null);

  const all = q.data?.processes ?? [];
  const f = FILTERS.find((x) => x.id === filter) ?? FILTERS[0];
  const t = text.trim().toLowerCase();
  const rows = all
    .filter(f.test)
    .filter((p) => !t || p.name.toLowerCase().includes(t) || String(p.pid) === t || (p.project?.name.toLowerCase().includes(t) ?? false) || p.ports.some((x) => String(x.port) === t))
    .sort((a, b) =>
      sort === "name" ? a.name.localeCompare(b.name) : sort === "cpu" ? (value(b.cpu_percent) ?? 0) - (value(a.cpu_percent) ?? 0) : b.memory_bytes - a.memory_bytes,
    );
  const sel = all.find((p) => p.pid === selected) ?? null;
  const totalMem = rows.reduce((s, p) => s + p.memory_bytes, 0);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title="Processes"
        subtitle={q.data ? `${formatNumber(all.length)} processes · listening ports: ${q.data.ports.status === "available" ? q.data.ports.value : unavailableReason(q.data.ports)}` : undefined}
      />
      <div className="flex shrink-0 flex-wrap items-center gap-2 border-b border-line px-5 py-2.5">
        <label className="flex h-7 w-64 items-center gap-1.5 rounded-md border border-line bg-surface px-2 focus-within:border-accent/60">
          <Search className="size-3.5 text-faint" aria-hidden />
          <input value={text} onChange={(e) => setText(e.target.value)} placeholder="Name, PID, port or project" aria-label="Search processes" className="min-w-0 flex-1 bg-transparent text-[12.5px] text-fg outline-none placeholder:text-faint" />
          {text ? (
            <button aria-label="Clear search" onClick={() => setText("")} className="text-faint hover:text-fg">
              <X className="size-3.5" />
            </button>
          ) : null}
        </label>
        {FILTERS.map((x) => (
          <button
            key={x.id}
            onClick={() => setFilter(x.id)}
            className={cn("h-6 rounded-full border px-2.5 text-[11.5px]", x.id === filter ? "border-accent/50 bg-accent/10 text-fg" : "border-line text-muted hover:text-fg")}
          >
            {x.label} <span className="text-faint">{all.filter(x.test).length}</span>
          </button>
        ))}
        <span className="ml-auto text-[11.5px] text-faint">
          {rows.length} shown · {formatBytes(totalMem)} memory · refreshes every 3 s
        </span>
      </div>
      <div className="flex min-h-0 flex-1">
        <div className="min-w-0 flex-1 overflow-auto">
          {q.isLoading ? (
            <StateView kind="loading" title="Reading the process table" />
          ) : q.error ? (
            <StateView kind="error" title="Could not read processes" description={errorMessage(q.error)} />
          ) : rows.length === 0 ? (
            <StateView kind="empty" title="No processes match" />
          ) : (
            <table className="w-full border-separate border-spacing-0 text-[12.5px]">
              <thead className="sticky top-0 z-[1] bg-bg">
                <tr className="text-left text-[11.5px] text-muted">
                  <th className="border-b border-line px-3 py-1.5 pl-5 font-medium">
                    <button onClick={() => setSort("name")} className={sort === "name" ? "text-fg" : ""}>Process</button>
                  </th>
                  <th className="border-b border-line px-3 py-1.5 font-medium">PID</th>
                  <th className="border-b border-line px-3 py-1.5 text-right font-medium">
                    <button onClick={() => setSort("cpu")} className={sort === "cpu" ? "text-fg" : ""}>CPU</button>
                  </th>
                  <th className="border-b border-line px-3 py-1.5 text-right font-medium">
                    <button onClick={() => setSort("memory")} className={sort === "memory" ? "text-fg" : ""}>Memory</button>
                  </th>
                  <th className="border-b border-line px-3 py-1.5 font-medium">Kind</th>
                  <th className="border-b border-line px-3 py-1.5 font-medium">Ports</th>
                  <th className="border-b border-line px-3 py-1.5 font-medium">Project</th>
                </tr>
              </thead>
              <tbody>
                {rows.slice(0, 500).map((p) => (
                  <tr key={p.pid} onClick={() => setSelected(p.pid === selected ? null : p.pid)} aria-selected={p.pid === selected} className={cn("cursor-pointer", p.pid === selected ? "bg-accent/10" : "hover:bg-hover/50")}>
                    <td className="max-w-[260px] truncate border-b border-line/60 px-3 py-1.5 pl-5 text-fg" title={p.exe ?? p.name}>
                      {p.protected ? <Lock className="mr-1 inline size-3 text-faint" aria-label="protected" /> : null}
                      {p.name}
                    </td>
                    <td className="tabular border-b border-line/60 px-3 py-1.5 text-faint">{p.pid}</td>
                    <td className="tabular border-b border-line/60 px-3 py-1.5 text-right">
                      <Avail value={p.cpu_percent} render={(c) => formatPercent(c, 1)} />
                    </td>
                    <td className="tabular border-b border-line/60 px-3 py-1.5 text-right">{formatBytes(p.memory_bytes)}</td>
                    <td className="border-b border-line/60 px-3 py-1.5 text-muted">{KIND_LABEL[p.kind]}</td>
                    <td className="max-w-[160px] truncate border-b border-line/60 px-3 py-1.5 font-mono text-[11.5px] text-muted">{p.ports.map((x) => x.port).join(", ")}</td>
                    <td className="max-w-[160px] truncate border-b border-line/60 px-3 py-1.5 text-muted">{p.project?.name ?? ""}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
          {rows.length > 500 ? <div className="px-5 py-2 text-[11.5px] text-faint">Showing the first 500; search to narrow.</div> : null}
        </div>
        {sel ? (
          <aside className="flex w-[380px] shrink-0 flex-col gap-4 overflow-y-auto border-l border-line bg-surface p-4" aria-label="Process details">
            <div className="flex items-start justify-between gap-2">
              <div>
                <Badge>{KIND_LABEL[sel.kind]}</Badge>
                <h2 className="selectable mt-1 text-[15px] font-semibold text-fg">{sel.name}</h2>
              </div>
              <Button size="icon" variant="ghost" aria-label="Close details" onClick={() => setSelected(null)}>
                <X />
              </Button>
            </div>
            <Kv
              items={[
                ["PID", `${sel.pid}${sel.parent_pid ? ` (parent ${sel.parent_pid})` : ""}`],
                ["CPU", <Avail value={sel.cpu_percent} render={(c) => formatPercent(c, 1)} />],
                ["Memory", formatBytes(sel.memory_bytes)],
                ["Running for", formatDuration(sel.run_time_secs)],
                ["Status", sel.status],
                ["Executable", sel.exe ? <span className="selectable break-all font-mono text-[11.5px]">{shortenHome(sel.exe)}</span> : "not visible"],
                ["Working dir", sel.cwd ? <span className="selectable break-all font-mono text-[11.5px]">{shortenHome(sel.cwd)}</span> : "not visible"],
                ["Project", sel.project ? `${sel.project.name} (via ${sel.project.via})` : "—"],
                ["Ports", sel.ports.length ? sel.ports.map((x) => `${x.address}:${x.port}`).join(", ") : "none seen"],
                ["App bundle", sel.app_bundle ? shortenHome(sel.app_bundle) : "—"],
              ]}
            />
            {sel.command.length ? (
              <div>
                <div className="mb-1 text-[11px] font-medium uppercase tracking-[0.07em] text-faint">Command line (redacted)</div>
                <code className="selectable block max-h-32 overflow-auto break-all rounded bg-raised p-2 font-mono text-[11px] text-fg">{sel.command.join(" ")}</code>
              </div>
            ) : null}
            <div className="flex flex-wrap gap-1.5">
              {sel.exe ? (
                <Button size="sm" onClick={() => void revealInFileManager(sel.exe as string)}>
                  <FolderOpen aria-hidden /> Reveal executable
                </Button>
              ) : null}
            </div>
            {sel.protected ? (
              <div className="flex gap-2 rounded-md bg-raised px-2.5 py-2 text-[12px] text-muted">
                <Lock className="mt-0.5 size-3.5 shrink-0" aria-hidden />
                AGASTHY will not stop this process: {sel.protected}.
              </div>
            ) : (
              <div className="flex flex-col gap-1.5 border-t border-line pt-3">
                <div className="text-[11px] font-medium uppercase tracking-[0.07em] text-faint">Stop</div>
                <div className="flex gap-1.5">
                  <Button size="sm" onClick={() => setAction({ type: "terminate_process", pid: sel.pid, start_time: sel.start_time, force: false })}>
                    <Square aria-hidden /> Stop…
                  </Button>
                  <Button size="sm" variant="danger" onClick={() => setAction({ type: "terminate_process", pid: sel.pid, start_time: sel.start_time, force: true })}>
                    <Zap aria-hidden /> Force quit…
                  </Button>
                </div>
                <p className="text-[11.5px] leading-5 text-faint">Both open a review first. Nothing happens until you confirm. Stopping a process frees its memory; it does not affect disk space.</p>
              </div>
            )}
          </aside>
        ) : null}
      </div>
      {action ? <ActionDialog request={action} onClose={() => setAction(null)} /> : null}
    </div>
  );
}
