import type {
  AiConfigFileAttrs,
  AiProviderAttrs,
  AiToolAttrs,
  Edge,
  Entity,
  EntityKind,
  LocalModelAttrs,
  McpServerAttrs,
} from "@agasthy/shared-types";
import { Badge, Button, StateView, cn } from "@agasthy/ui";
import { useQuery } from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { ArrowRight, Network, RefreshCw, Search, X } from "lucide-react";
import { useState, type ReactNode } from "react";
import { PROVIDER_LEVEL } from "../components/ai-attributes";
import { PageHeader, Panel, Stat } from "../components/page";
import { progressLabel, useScanActivity } from "../components/scan-activity";
import { formatBytes, formatNumber, formatRelative, shortenHome } from "../lib/format";
import { api, errorMessage } from "../lib/ipc";
import { useEntities, useOverview } from "../lib/queries";
import { EntityDetailPanel } from "./entity-detail";

export type AiTab = "overview" | "assistants" | "providers" | "mcp" | "instructions" | "models";

export interface AiSearch {
  tab: AiTab;
  id?: string;
  q?: string;
  filter?: string;
}

export const AI_TABS: { id: AiTab; label: string; kind: EntityKind | null }[] = [
  { id: "overview", label: "Overview", kind: null },
  { id: "assistants", label: "Assistants & engines", kind: "ai_tool" },
  { id: "providers", label: "Providers", kind: "ai_provider" },
  { id: "mcp", label: "MCP servers", kind: "mcp_server" },
  { id: "instructions", label: "Instructions & config", kind: "ai_config_file" },
  { id: "models", label: "Local models", kind: "local_model" },
];

const A = <T,>(e: Entity) => e.attributes as unknown as T;

function useEdges(kind: "potential_conflict" | "same_content") {
  return useQuery({ queryKey: ["graph", "edges", kind], queryFn: () => api.listEdges(kind) });
}

interface Col {
  header: string;
  cell: (e: Entity) => ReactNode;
  className?: string;
}

const COLS: Record<Exclude<AiTab, "overview">, Col[]> = {
  assistants: [
    { header: "Tool", cell: (e) => <span className="font-medium text-fg">{e.name}</span> },
    { header: "Vendor", cell: (e) => <span className="text-muted">{A<AiToolAttrs>(e).vendor}</span> },
    { header: "Kind", cell: (e) => <span className="text-muted">{A<AiToolAttrs>(e).category.replaceAll("_", " ")}</span> },
    {
      header: "Status",
      cell: (e) => {
        const t = A<AiToolAttrs>(e);
        if (!t.installed) return <Badge tone="warning" title="Only configuration was found">config only</Badge>;
        if (t.local_endpoint) return t.local_endpoint.listening ? <Badge tone="success">running</Badge> : <Badge>installed · not running</Badge>;
        return <Badge tone="success">installed</Badge>;
      },
    },
    { header: "Version", cell: (e) => <span className="font-mono text-[11.5px]">{e.version ?? "—"}</span> },
    { header: "Evidence", cell: (e) => <span className="text-faint">{[...new Set(A<AiToolAttrs>(e).signals.map((s) => s.kind.replace("_", " ")))].join(", ")}</span> },
  ],
  providers: [
    { header: "Provider", cell: (e) => <span className="font-medium text-fg">{e.name}</span> },
    { header: "Type", cell: (e) => <span className="text-muted">{A<AiProviderAttrs>(e).kind}</span> },
    {
      header: "Strongest evidence",
      cell: (e) => {
        const l = A<AiProviderAttrs>(e).highest;
        return (
          <Badge tone={PROVIDER_LEVEL[l].tone} title={PROVIDER_LEVEL[l].hint}>
            {PROVIDER_LEVEL[l].label}
          </Badge>
        );
      },
    },
    { header: "Signals", cell: (e) => <span className="text-faint">{A<AiProviderAttrs>(e).signals.length}</span>, className: "tabular" },
  ],
  mcp: [
    { header: "Server", cell: (e) => <span className="font-medium text-fg">{e.name}</span> },
    { header: "Transport", cell: (e) => <span className="text-muted">{A<McpServerAttrs>(e).transport}</span> },
    {
      header: "Command / URL",
      cell: (e) => {
        const s = A<McpServerAttrs>(e);
        return <span className="font-mono text-[11.5px] text-muted">{s.command ? shortenHome(s.command.split("/").pop() ?? s.command) + " " + s.args.slice(0, 3).join(" ") : (s.url ?? "—")}</span>;
      },
    },
    { header: "For", cell: (e) => A<McpServerAttrs>(e).tool ?? "—" },
    { header: "Scope", cell: (e) => (A<McpServerAttrs>(e).scope === "global" ? "all projects" : <span title={A<McpServerAttrs>(e).scope}>one project</span>) },
    { header: "State", cell: (e) => (A<McpServerAttrs>(e).disabled ? <Badge>disabled</Badge> : <Badge tone="success">enabled</Badge>) },
  ],
  instructions: [
    { header: "File", cell: (e) => <span className="font-medium text-fg">{e.name}</span> },
    { header: "Type", cell: (e) => <span className="text-muted">{A<AiConfigFileAttrs>(e).file_type.replace("_", " ")}</span> },
    { header: "Scope", cell: (e) => <span className="text-muted">{A<AiConfigFileAttrs>(e).scope}</span> },
    { header: "Project", cell: (e) => <span className="text-muted">{A<AiConfigFileAttrs>(e).project?.split("/").pop() ?? "—"}</span> },
    { header: "Read by", cell: (e) => <span className="text-faint">{A<AiConfigFileAttrs>(e).tools.slice(0, 3).join(", ")}{A<AiConfigFileAttrs>(e).tools.length > 3 ? "…" : ""}</span> },
    {
      header: "Flags",
      cell: (e) => {
        const f = A<AiConfigFileAttrs>(e);
        return (
          <span className="flex gap-1">
            {f.orphaned ? <Badge tone="warning">orphaned</Badge> : null}
            {f.duplicates.length ? <Badge tone="info">duplicate</Badge> : null}
            {f.overlaps.length ? <Badge>overlap</Badge> : null}
          </span>
        );
      },
    },
    { header: "Modified", cell: (e) => <span className="text-faint">{A<AiConfigFileAttrs>(e).modified_at ? formatRelative(A<AiConfigFileAttrs>(e).modified_at as number) : "—"}</span> },
  ],
  models: [
    { header: "Model", cell: (e) => <span className="font-medium text-fg">{e.name}</span> },
    { header: "Tag / file", cell: (e) => <span className="font-mono text-[11.5px] text-muted">{A<LocalModelAttrs>(e).tag ?? "—"}</span> },
    { header: "Store", cell: (e) => <span className="text-muted">{A<LocalModelAttrs>(e).store}</span> },
    { header: "Size", cell: (e) => (A<LocalModelAttrs>(e).size_bytes != null ? formatBytes(A<LocalModelAttrs>(e).size_bytes as number) : "—"), className: "text-right tabular" },
  ],
};

interface Filter {
  id: string;
  label: string;
  test: (e: Entity, ctx: { conflicted: Set<string> }) => boolean;
}

function filtersFor(tab: Exclude<AiTab, "overview">, rows: Entity[]): Filter[] {
  const all: Filter = { id: "all", label: "All", test: () => true };
  switch (tab) {
    case "assistants":
      return [
        all,
        { id: "installed", label: "Installed", test: (e) => A<AiToolAttrs>(e).installed },
        { id: "config-only", label: "Config only", test: (e) => !A<AiToolAttrs>(e).installed },
        { id: "local", label: "Local inference", test: (e) => A<AiToolAttrs>(e).category === "local_inference" },
      ];
    case "providers":
      return [all, { id: "cloud", label: "Cloud", test: (e) => A<AiProviderAttrs>(e).kind === "cloud" }, { id: "local", label: "Local", test: (e) => A<AiProviderAttrs>(e).kind === "local" }];
    case "mcp":
      return [all, { id: "global", label: "All projects", test: (e) => A<McpServerAttrs>(e).scope === "global" }, { id: "project", label: "Project-scoped", test: (e) => A<McpServerAttrs>(e).scope !== "global" }];
    case "instructions": {
      const tools = [...new Set(rows.flatMap((e) => A<AiConfigFileAttrs>(e).tools))].sort();
      return [
        all,
        { id: "instructions", label: "Instructions & rules", test: (e) => ["instructions", "rules"].includes(A<AiConfigFileAttrs>(e).file_type) },
        { id: "agents", label: "Agents & skills", test: (e) => ["agent", "skill", "command"].includes(A<AiConfigFileAttrs>(e).file_type) },
        { id: "mcp-config", label: "MCP & settings", test: (e) => ["mcp_config", "settings"].includes(A<AiConfigFileAttrs>(e).file_type) },
        { id: "conflicts", label: "Potential conflicts", test: (e, c) => c.conflicted.has(e.id) },
        { id: "duplicates", label: "Duplicates", test: (e) => A<AiConfigFileAttrs>(e).duplicates.length > 0 },
        { id: "orphaned", label: "Orphaned", test: (e) => A<AiConfigFileAttrs>(e).orphaned },
        ...tools.map((t) => ({ id: `tool:${t}`, label: t, test: (e: Entity) => A<AiConfigFileAttrs>(e).tools.includes(t) })),
      ];
    }
    case "models":
      return [all];
  }
}

export function AiPage() {
  const search = useSearch({ from: "/ai" });
  const nav = useNavigate({ from: "/ai" });
  const activity = useScanActivity();
  const overview = useOverview();
  const running = activity.running.has("ai");
  const scan = overview.data?.latest_scans.find((s) => s.kind === "ai");

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title="AI Ecosystem"
        subtitle={scan ? `last scanned ${formatRelative(scan.finished_at ?? scan.started_at)}` : "not scanned yet"}
        actions={
          <Button size="sm" disabled={running} onClick={() => void activity.start("ai")}>
            <RefreshCw className={running ? "animate-spin" : ""} aria-hidden />
            {running ? progressLabel(activity.progress.ai) : "Scan AI ecosystem"}
          </Button>
        }
      />
      <div className="flex shrink-0 items-center gap-1 border-b border-line px-5 py-2" role="tablist">
        {AI_TABS.map((t) => (
          <button
            key={t.id}
            role="tab"
            aria-selected={t.id === search.tab}
            onClick={() => void nav({ search: { tab: t.id } })}
            className={cn("h-7 rounded-md px-2.5 text-[12.5px] transition-colors", t.id === search.tab ? "bg-hover text-fg" : "text-muted hover:text-fg")}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div className="flex min-h-0 flex-1">
        <div className="min-w-0 flex-1 overflow-auto">
          {activity.lastError.ai ? <div className="mx-5 mt-3 rounded-md bg-danger/10 px-2.5 py-1.5 text-[12px] text-danger">{activity.lastError.ai}</div> : null}
          {!scan && !running ? (
            <StateView
              kind="empty"
              title="AI ecosystem not scanned yet"
              description="AGASTHY reads fixed locations in your home folder (such as ~/.claude, ~/.codex, ~/.cursor) and AI files inside repositories you have scanned. Credential files are only checked for existence; their contents are never read."
              action={
                <Button variant="primary" onClick={() => void activity.start("ai")}>
                  Scan AI ecosystem
                </Button>
              }
            />
          ) : search.tab === "overview" ? (
            <AiOverview />
          ) : (
            <AiList key={`${search.tab}:${search.q ?? ""}`} tab={search.tab} />
          )}
        </div>
        {search.id ? (
          <EntityDetailPanel
            id={search.id}
            onClose={() => void nav({ search: (s) => ({ ...s, id: undefined }) })}
            onNavigate={(id) => void nav({ search: (s) => ({ ...s, id }) })}
            actions={
              <Button size="sm" variant="ghost" onClick={() => void nav({ to: "/graph", search: { view: "focus", focus: search.id, depth: 1, sel: search.id } })}>
                <Network aria-hidden /> Open in graph
              </Button>
            }
          />
        ) : null}
      </div>
    </div>
  );
}

const QUESTIONS: { q: string; tab: AiTab; filter?: string; text?: string }[] = [
  { q: "Which AI coding assistants are installed?", tab: "assistants", filter: "installed" },
  { q: "Where are my AGENTS.md files?", tab: "instructions", text: "AGENTS" },
  { q: "Which projects use Claude Code?", tab: "instructions", filter: "tool:claude-code" },
  { q: "Which MCP servers are configured?", tab: "mcp" },
  { q: "Which local models are available?", tab: "models" },
  { q: "Do I have conflicting AI instructions?", tab: "instructions", filter: "conflicts" },
  { q: "Which AI tools are using cloud providers?", tab: "providers", filter: "cloud" },
];

function AiOverview() {
  const nav = useNavigate({ from: "/ai" });
  const tools = useEntities("ai_tool", "");
  const providers = useEntities("ai_provider", "");
  const mcp = useEntities("mcp_server", "");
  const files = useEntities("ai_config_file", "");
  const models = useEntities("local_model", "");
  const conflicts = useEdges("potential_conflict");
  const loading = [tools, providers, mcp, files, models].some((q) => q.isLoading);
  const error = [tools, providers, mcp, files, models].find((q) => q.error)?.error;
  if (loading) return <StateView kind="loading" title="Loading" />;
  if (error) return <StateView kind="error" title="Could not load AI data" description={errorMessage(error)} />;

  const t = tools.data ?? [];
  const f = files.data ?? [];
  const installed = t.filter((e) => A<AiToolAttrs>(e).installed);
  const uniqueServers = new Set((mcp.data ?? []).map((e) => A<McpServerAttrs>(e).fingerprint)).size;
  const instr = f.filter((e) => ["instructions", "rules"].includes(A<AiConfigFileAttrs>(e).file_type));
  const agents = f.filter((e) => ["agent", "skill", "command"].includes(A<AiConfigFileAttrs>(e).file_type));
  const go = (tab: AiTab, filter?: string, q?: string) => void nav({ search: { tab, filter, q } });
  const attention: { text: string; run: () => void }[] = [];
  const nConflicts = (conflicts.data ?? []).length;
  if (nConflicts) attention.push({ text: `${nConflicts} potential instruction conflict${nConflicts === 1 ? "" : "s"}`, run: () => go("instructions", "conflicts") });
  const dups = f.filter((e) => A<AiConfigFileAttrs>(e).duplicates.length).length;
  if (dups) attention.push({ text: `${dups} file${dups === 1 ? "" : "s"} with identical content elsewhere`, run: () => go("instructions", "duplicates") });
  const orphaned = f.filter((e) => A<AiConfigFileAttrs>(e).orphaned).length;
  if (orphaned)
    attention.push({ text: `${orphaned} file${orphaned === 1 ? "" : "s"} for tools that are not installed`, run: () => go("instructions", "orphaned") });
  const configOnly = t.length - installed.length;
  if (configOnly)
    attention.push({
      text: `${configOnly} tool${configOnly === 1 ? "" : "s"} with configuration but no installation`,
      run: () => go("assistants", "config-only"),
    });

  return (
    <div className="flex flex-col gap-4 p-5">
      <Panel title="At a glance" bodyClassName="grid grid-cols-2 gap-1 p-1.5 md:grid-cols-3 xl:grid-cols-6">
        <Stat label="AI tools installed" value={formatNumber(installed.length)} hint={configOnly ? `${configOnly} config only` : "assistants, apps, engines"} onClick={() => go("assistants")} />
        <Stat label="Providers" value={formatNumber((providers.data ?? []).length)} hint="referenced or configured" onClick={() => go("providers")} />
        <Stat label="MCP servers" value={formatNumber((mcp.data ?? []).length)} hint={`${uniqueServers} distinct`} onClick={() => go("mcp")} />
        <Stat label="Instruction files" value={formatNumber(instr.length)} hint="instructions & rules" onClick={() => go("instructions", "instructions")} />
        <Stat label="Agents, skills, commands" value={formatNumber(agents.length)} onClick={() => go("instructions", "agents")} />
        <Stat label="Local models" value={formatNumber((models.data ?? []).length)} onClick={() => go("models")} />
      </Panel>
      <div className="grid grid-cols-1 gap-4 xl:grid-cols-2">
        <Panel title="Ask about your AI setup">
          <ul className="divide-y divide-line">
            {QUESTIONS.map((x) => (
              <li key={x.q}>
                <button className="flex w-full items-center justify-between gap-2 py-1.5 text-left text-[12.5px] text-fg hover:text-accent" onClick={() => go(x.tab, x.filter, x.text)}>
                  {x.q}
                  <ArrowRight className="size-3.5 text-faint" aria-hidden />
                </button>
              </li>
            ))}
          </ul>
        </Panel>
        <Panel title="Needs attention">
          {attention.length === 0 ? (
            <StateView kind="empty" title="Nothing flagged" description="No conflicts, duplicates or orphaned files were found." compact />
          ) : (
            <ul className="divide-y divide-line">
              {attention.map((a) => (
                <li key={a.text} className="flex items-center justify-between gap-2 py-1.5">
                  <span className="text-[12.5px] text-fg">{a.text}</span>
                  <Button size="sm" variant="ghost" onClick={a.run}>
                    Show <ArrowRight aria-hidden />
                  </Button>
                </li>
              ))}
            </ul>
          )}
          <p className="mt-2 text-[11.5px] leading-5 text-faint">
            Conflicts are detected for a few explicit settings (package manager, indentation, quote style) stated in files that apply together. They are
            hints with evidence, not judgements.
          </p>
        </Panel>
      </div>
    </div>
  );
}

function AiList({ tab }: { tab: Exclude<AiTab, "overview"> }) {
  const search = useSearch({ from: "/ai" });
  const nav = useNavigate({ from: "/ai" });
  const kind = AI_TABS.find((t) => t.id === tab)?.kind as EntityKind;
  const [text, setText] = useState(search.q ?? "");
  const q = useEntities(kind, text);
  const conflicts = useEdges("potential_conflict");
  const conflicted = new Set((conflicts.data ?? []).flatMap((e: Edge) => [e.from_id, e.to_id]));
  const rows = q.data ?? [];
  const filters = filtersFor(tab, rows);
  const filter = filters.find((f) => f.id === search.filter) ?? filters[0];
  const visible = rows.filter((e) => filter.test(e, { conflicted }));
  const cols = COLS[tab];
  return (
    <div className="flex flex-col">
      <div className="flex flex-wrap items-center gap-2 border-b border-line px-5 py-2.5">
        <label className="flex h-7 w-72 items-center gap-1.5 rounded-md border border-line bg-surface px-2 focus-within:border-accent/60">
          <Search className="size-3.5 text-faint" aria-hidden />
          <input
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder={tab === "instructions" ? "Search file names and contents" : "Search"}
            aria-label="Search"
            className="min-w-0 flex-1 bg-transparent text-[12.5px] text-fg outline-none placeholder:text-faint"
          />
          {text ? (
            <button aria-label="Clear search" onClick={() => setText("")} className="text-faint hover:text-fg">
              <X className="size-3.5" />
            </button>
          ) : null}
        </label>
        {filters.length > 1
          ? filters.map((f) => (
              <button
                key={f.id}
                onClick={() => void nav({ search: (s) => ({ ...s, filter: f.id === "all" ? undefined : f.id }) })}
                className={cn("h-6 rounded-full border px-2.5 text-[11.5px]", f.id === filter.id ? "border-accent/50 bg-accent/10 text-fg" : "border-line text-muted hover:text-fg")}
              >
                {f.label}
                {f.id !== "all" ? <span className="ml-1 text-faint">{rows.filter((e) => f.test(e, { conflicted })).length}</span> : null}
              </button>
            ))
          : null}
        <span className="ml-auto text-[11.5px] text-faint tabular">{formatNumber(visible.length)}</span>
      </div>
      {q.isLoading ? (
        <StateView kind="loading" title="Loading" />
      ) : q.error ? (
        <StateView kind="error" title="Could not load" description={errorMessage(q.error)} />
      ) : visible.length === 0 ? (
        <StateView kind="empty" title={text ? `No matches for “${text}”` : rows.length ? "Nothing matches this filter" : "None found on this machine"} />
      ) : (
        <table className="w-full border-separate border-spacing-0 text-[12.5px]">
          <thead className="sticky top-0 z-[1] bg-bg">
            <tr>
              {cols.map((c) => (
                <th key={c.header} className={cn("border-b border-line px-3 py-1.5 text-left text-[11.5px] font-medium text-muted first:pl-5", c.className)}>
                  {c.header}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {visible.map((e) => (
              <tr
                key={e.id}
                onClick={() => void nav({ search: (s) => ({ ...s, id: s.id === e.id ? undefined : e.id }) })}
                aria-selected={search.id === e.id}
                className={cn("cursor-pointer", search.id === e.id ? "bg-accent/10" : "hover:bg-hover/50")}
              >
                {cols.map((c) => (
                  <td key={c.header} className={cn("max-w-[320px] truncate border-b border-line/60 px-3 py-1.5 first:pl-5", c.className)}>
                    {c.cell(e)}
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </div>
  );
}
