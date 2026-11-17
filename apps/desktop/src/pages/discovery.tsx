import type {
  BinaryAttrs,
  DependencyAttrs,
  Entity,
  EntityKind,
  InstalledApp,
  PackageAttrs,
  PackageManagerAttrs,
  RepositoryAttrs,
  ScanKind,
} from "@agasthy/shared-types";
import { Badge, Button, StateView, cn } from "@agasthy/ui";
import { useQueries } from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { ArrowDown, ArrowUp, FolderPlus, Network, RefreshCw, Search, X } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { Avail } from "../components/availability";
import { RuntimeImpact } from "../components/impact";
import { PageHeader } from "../components/page";
import { progressLabel, useScanActivity } from "../components/scan-activity";
import { formatBytes, formatNumber, formatRelative, shortenHome, value } from "../lib/format";
import { api, errorMessage } from "../lib/ipc";
import { keys, useOverview } from "../lib/queries";
import { EntityDetailPanel } from "./entity-detail";

export type DiscoveryTab = "repositories" | "applications" | "tools" | "packages" | "managers" | "dependencies";

export const TABS: { id: DiscoveryTab; label: string; kinds: EntityKind[]; scan: ScanKind }[] = [
  { id: "repositories", label: "Repositories", kinds: ["repository"], scan: "repositories" },
  { id: "applications", label: "Applications", kinds: ["application"], scan: "software" },
  { id: "tools", label: "CLI tools & runtimes", kinds: ["runtime", "binary"], scan: "software" },
  { id: "packages", label: "Packages", kinds: ["package"], scan: "packages" },
  { id: "managers", label: "Package managers", kinds: ["package_manager"], scan: "packages" },
  { id: "dependencies", label: "Project dependencies", kinds: ["dependency"], scan: "repositories" },
];

/** What each entity kind *is*. Provenance classes are never merged. */
export const KIND_LABEL: Record<EntityKind, { short: string; provenance: string }> = {
  repository: { short: "Repo", provenance: "Source repository cloned locally" },
  remote_project: { short: "Upstream", provenance: "Upstream project named by a remote or package metadata" },
  application: { short: "App", provenance: "Application installed on this machine" },
  binary: { short: "Tool", provenance: "Executable found on PATH" },
  runtime: { short: "Runtime", provenance: "Language runtime found on PATH" },
  package: { short: "Package", provenance: "Installed through a package manager" },
  package_manager: { short: "Manager", provenance: "Package manager whose inventory AGASTHY read" },
  dependency: { short: "Dependency", provenance: "Declared in a project manifest (not necessarily installed)" },
  ai_tool: { short: "AI tool", provenance: "AI assistant or engine found on this machine" },
  ai_provider: { short: "Provider", provenance: "AI provider referenced or configured on this machine" },
  local_model: { short: "Model", provenance: "Model stored on disk for local inference" },
  mcp_server: { short: "MCP", provenance: "MCP server declared in a configuration file" },
  ai_config_file: { short: "AI file", provenance: "AI instruction or configuration file" },
  finding: { short: "Finding", provenance: "Security or lifecycle finding" },
};

/** Kinds shown on the AI Ecosystem page rather than in Discovery. */
export const AI_KINDS: EntityKind[] = ["ai_tool", "ai_provider", "local_model", "mcp_server", "ai_config_file"];

export function tabForKind(kind: EntityKind): DiscoveryTab {
  return TABS.find((t) => t.kinds.includes(kind))?.id ?? "repositories";
}

const attrs = <T,>(e: Entity) => e.attributes as unknown as T;

interface Column {
  id: string;
  header: string;
  cell: (e: Entity) => ReactNode;
  sort?: (e: Entity) => string | number;
  className?: string;
}

interface Filter {
  id: string;
  label: string;
  test: (e: Entity) => boolean;
}

function repoStatus(e: Entity): ReactNode {
  const a = attrs<RepositoryAttrs>(e);
  return (
    <Avail
      value={a.working_tree}
      render={(w) =>
        w.clean ? (
          <span className="text-faint">clean</span>
        ) : (
          <span className="text-warning">
            {[w.staged && `${w.staged} staged`, w.modified && `${w.modified} modified`, w.untracked && `${w.untracked} new`, w.conflicted && `${w.conflicted} conflicts`]
              .filter(Boolean)
              .join(" · ")}
          </span>
        )
      }
    />
  );
}

const COLUMNS: Record<DiscoveryTab, Column[]> = {
  repositories: [
    { id: "name", header: "Repository", cell: (e) => <span className="font-medium text-fg">{e.name}</span>, sort: (e) => e.name.toLowerCase() },
    {
      id: "branch",
      header: "Branch",
      cell: (e) => (
        <Avail
          value={attrs<RepositoryAttrs>(e).head}
          render={(h) => (h.unborn ? <span className="text-faint">no commits</span> : h.detached ? <span className="text-warning">detached</span> : <span className="font-mono text-[11.5px]">{h.branch}</span>)}
        />
      ),
      sort: (e) => value(attrs<RepositoryAttrs>(e).head)?.branch ?? "",
    },
    { id: "status", header: "Working tree", cell: repoStatus, sort: (e) => (value(attrs<RepositoryAttrs>(e).working_tree)?.clean ? 1 : 0) },
    {
      id: "commit",
      header: "Last commit",
      cell: (e) => {
        const t = value(attrs<RepositoryAttrs>(e).head)?.last_commit_at;
        return t ? <span title={new Date(t).toLocaleString()}>{formatRelative(t)}</span> : <span className="text-faint">—</span>;
      },
      sort: (e) => value(attrs<RepositoryAttrs>(e).head)?.last_commit_at ?? 0,
    },
    {
      id: "remote",
      header: "Primary remote",
      cell: (e) => {
        const a = attrs<RepositoryAttrs>(e);
        const r = a.primary_remote;
        if (!r) return <span className="text-faint">{a.remotes.length ? "unparsed" : "none"}</span>;
        return (
          <span className="flex items-center gap-1.5">
            <span className="truncate">{r.is_github ? `${r.owner}/${r.name}` : `${r.host}/${r.path}`}</span>
            {a.clones_of_same_remote > 1 ? <Badge tone="warning">{a.clones_of_same_remote} clones</Badge> : null}
          </span>
        );
      },
      sort: (e) => attrs<RepositoryAttrs>(e).primary_remote?.path.toLowerCase() ?? "~",
    },
    { id: "lang", header: "Language", cell: (e) => attrs<RepositoryAttrs>(e).primary_language ?? <span className="text-faint">—</span>, sort: (e) => attrs<RepositoryAttrs>(e).primary_language ?? "~" },
    {
      id: "size",
      header: "Size",
      cell: (e) => <Avail value={attrs<RepositoryAttrs>(e).size_bytes} render={(b) => formatBytes(b)} />,
      sort: (e) => value(attrs<RepositoryAttrs>(e).size_bytes) ?? -1,
      className: "text-right tabular",
    },
    { id: "path", header: "Path", cell: (e) => <span className="text-faint">{shortenHome(e.path ?? "")}</span>, sort: (e) => e.path ?? "" },
  ],
  applications: [
    { id: "name", header: "Application", cell: (e) => <span className="font-medium text-fg">{e.name}</span>, sort: (e) => e.name.toLowerCase() },
    { id: "version", header: "Version", cell: (e) => e.version ?? <span className="text-faint">—</span>, sort: (e) => e.version ?? "" },
    { id: "bundle", header: "Bundle identifier", cell: (e) => <span className="font-mono text-[11.5px] text-muted">{attrs<InstalledApp>(e).bundle_id ?? "—"}</span>, sort: (e) => attrs<InstalledApp>(e).bundle_id ?? "" },
    {
      id: "location",
      header: "Location",
      cell: (e) => {
        const l = attrs<InstalledApp>(e).location;
        return <Badge tone={l === "system" ? "neutral" : l === "user" ? "info" : "accent"}>{l === "system" ? "system" : l === "user" ? "user" : "all users"}</Badge>;
      },
      sort: (e) => attrs<InstalledApp>(e).location,
    },
    { id: "path", header: "Path", cell: (e) => <span className="text-faint">{shortenHome(e.path ?? "")}</span>, sort: (e) => e.path ?? "" },
  ],
  tools: [
    { id: "name", header: "Tool", cell: (e) => <span className="font-medium text-fg">{e.name}</span>, sort: (e) => e.name.toLowerCase() },
    { id: "version", header: "Version", cell: (e) => <Avail value={attrs<BinaryAttrs>(e).version} render={(v) => <span className="font-mono text-[11.5px]">{v}</span>} />, sort: (e) => e.version ?? "" },
    { id: "category", header: "Category", cell: (e) => <span className="text-muted">{attrs<BinaryAttrs>(e).category.replace("_", " ")}</span>, sort: (e) => attrs<BinaryAttrs>(e).category },
    {
      id: "active",
      header: "Resolution",
      cell: (e) => (attrs<BinaryAttrs>(e).active ? <Badge tone="success">active</Badge> : <Badge title={`shadowed by ${attrs<BinaryAttrs>(e).shadowed_by ?? ""}`}>shadowed</Badge>),
      sort: (e) => (attrs<BinaryAttrs>(e).active ? 0 : 1),
    },
    { id: "via", header: "Installed via", cell: (e) => attrs<BinaryAttrs>(e).install_hint?.installer ?? <span className="text-faint">unknown</span>, sort: (e) => attrs<BinaryAttrs>(e).install_hint?.installer ?? "~" },
    { id: "path", header: "Path", cell: (e) => <span className="text-faint">{shortenHome(e.path ?? "")}</span>, sort: (e) => e.path ?? "" },
  ],
  packages: [
    { id: "name", header: "Package", cell: (e) => <span className="font-medium text-fg">{e.name}</span>, sort: (e) => e.name.toLowerCase() },
    { id: "version", header: "Version", cell: (e) => <span className="font-mono text-[11.5px]">{e.version ?? "—"}</span>, sort: (e) => e.version ?? "" },
    { id: "manager", header: "Manager", cell: (e) => attrs<PackageAttrs>(e).manager, sort: (e) => attrs<PackageAttrs>(e).manager },
    { id: "kind", header: "Type", cell: (e) => <span className="text-muted">{attrs<PackageAttrs>(e).package_kind}</span>, sort: (e) => attrs<PackageAttrs>(e).package_kind },
    {
      id: "upstream",
      header: "Upstream",
      cell: (e) => {
        const u = attrs<PackageAttrs>(e).upstream;
        return u ? `${u.owner}/${u.name}` : <span className="text-faint">—</span>;
      },
      sort: (e) => attrs<PackageAttrs>(e).upstream?.path ?? "~",
    },
    { id: "desc", header: "Description", cell: (e) => <span className="text-muted">{attrs<PackageAttrs>(e).description ?? ""}</span> },
  ],
  dependencies: [
    { id: "name", header: "Dependency", cell: (e) => <span className="font-medium text-fg">{e.name}</span>, sort: (e) => e.name.toLowerCase() },
    { id: "eco", header: "Ecosystem", cell: (e) => <span className="text-muted">{attrs<DependencyAttrs>(e).ecosystem}</span>, sort: (e) => attrs<DependencyAttrs>(e).ecosystem },
  ],
  managers: [
    { id: "name", header: "Package manager", cell: (e) => <span className="font-medium text-fg">{e.name}</span>, sort: (e) => e.name.toLowerCase() },
    {
      id: "state",
      header: "Inventory",
      cell: (e) => {
        const s = attrs<PackageManagerAttrs>(e).state;
        if (s.state === "read") return <Badge tone="success">read</Badge>;
        if (s.state === "not_installed") return <Badge>not installed</Badge>;
        if (s.state === "not_implemented") return <Badge tone="warning">installed · not supported yet</Badge>;
        return (
          <Badge tone="danger" title={s.message}>
            failed
          </Badge>
        );
      },
      sort: (e) => attrs<PackageManagerAttrs>(e).state.state,
    },
    {
      id: "count",
      header: "Packages",
      cell: (e) => {
        const c = attrs<PackageManagerAttrs>(e).package_count;
        return c == null ? <span className="text-faint">—</span> : formatNumber(c);
      },
      sort: (e) => attrs<PackageManagerAttrs>(e).package_count ?? -1,
      className: "text-right tabular",
    },
    { id: "version", header: "Version", cell: (e) => e.version ?? <span className="text-faint">—</span> },
    { id: "path", header: "Executable", cell: (e) => <span className="text-faint">{e.path ? shortenHome(e.path) : "—"}</span> },
  ],
};

function filtersFor(tab: DiscoveryTab, rows: Entity[]): Filter[] {
  const all: Filter = { id: "all", label: "All", test: () => true };
  switch (tab) {
    case "repositories":
      return [
        all,
        { id: "dirty", label: "Uncommitted changes", test: (e) => value(attrs<RepositoryAttrs>(e).working_tree)?.clean === false },
        { id: "duplicates", label: "Duplicate clones", test: (e) => attrs<RepositoryAttrs>(e).clones_of_same_remote > 1 },
        { id: "github", label: "GitHub", test: (e) => Boolean(attrs<RepositoryAttrs>(e).primary_remote?.is_github) },
        { id: "no-remote", label: "No remote", test: (e) => attrs<RepositoryAttrs>(e).remotes.length === 0 },
      ];
    case "applications":
      return [all, { id: "installed", label: "Not shipped with macOS", test: (e) => attrs<InstalledApp>(e).location !== "system" }];
    case "tools":
      return [
        all,
        { id: "runtimes", label: "Runtimes", test: (e) => e.kind === "runtime" },
        { id: "active", label: "Active", test: (e) => attrs<BinaryAttrs>(e).active },
        { id: "shadowed", label: "Shadowed copies", test: (e) => !attrs<BinaryAttrs>(e).active },
        { id: "ai", label: "AI tools", test: (e) => attrs<BinaryAttrs>(e).category === "ai_tool" },
      ];
    case "packages": {
      const managers = [...new Set(rows.map((e) => attrs<PackageAttrs>(e).manager))].sort();
      return [all, ...managers.map((m) => ({ id: m, label: m, test: (e: Entity) => attrs<PackageAttrs>(e).manager === m }))];
    }
    case "managers":
      return [all, { id: "read", label: "Read", test: (e) => attrs<PackageManagerAttrs>(e).state.state === "read" }];
    case "dependencies": {
      const ecos = [...new Set(rows.map((e) => attrs<DependencyAttrs>(e).ecosystem))].sort();
      return [all, ...ecos.map((x) => ({ id: x, label: x, test: (e: Entity) => attrs<DependencyAttrs>(e).ecosystem === x }))];
    }
  }
}

function useDebounced<T>(v: T, ms: number): T {
  const [d, setD] = useState(v);
  useEffect(() => {
    const t = setTimeout(() => setD(v), ms);
    return () => clearTimeout(t);
  }, [v, ms]);
  return d;
}

export function DiscoveryPage() {
  const search = useSearch({ from: "/discovery" });
  const nav = useNavigate({ from: "/discovery" });
  const tab = TABS.find((t) => t.id === search.tab) ?? TABS[0];
  const [text, setText] = useState(search.q ?? "");
  const q = useDebounced(text, 200);
  // Sort order is remembered per tab.
  const [sorts, setSorts] = useState<Partial<Record<DiscoveryTab, { col: string; desc: boolean }>>>({});
  const sort = sorts[tab.id] ?? { col: "name", desc: false };
  const setSort = (f: (s: { col: string; desc: boolean }) => { col: string; desc: boolean }) =>
    setSorts((all) => ({ ...all, [tab.id]: f(all[tab.id] ?? { col: "name", desc: false }) }));
  const overview = useOverview();
  const activity = useScanActivity();

  useEffect(() => {
    if ((search.q ?? "") !== q) void nav({ search: (s) => ({ ...s, q: q || undefined }), replace: true });
  }, [q, search.q, nav]);

  const results = useQueries({
    queries: tab.kinds.map((kind) => ({
      queryKey: keys.entities(kind, q),
      queryFn: () => api.listEntities({ kind, text: q.trim() || null, limit: 5000, offset: null }),
      placeholderData: (prev: Entity[] | undefined) => prev,
    })),
  });
  const loading = results.some((r) => r.isLoading);
  const error = results.find((r) => r.error)?.error;
  const rows = results.flatMap((r) => r.data ?? []);

  const filters = filtersFor(tab.id, rows);
  const filter = filters.find((f) => f.id === search.filter) ?? filters[0];
  const columns = COLUMNS[tab.id];

  const sortCol = columns.find((c) => c.id === sort.col) ?? columns[0];
  const filtered = rows.filter(filter.test);
  const sortKey = sortCol.sort;
  const visible = sortKey
    ? [...filtered].sort((a, b) => {
        const x = sortKey(a);
        const y = sortKey(b);
        const c = typeof x === "number" && typeof y === "number" ? x - y : String(x).localeCompare(String(y));
        return sort.desc ? -c : c;
      })
    : filtered;

  const scan = overview.data?.latest_scans.find((s) => s.kind === tab.scan);
  const running = activity.running.has(tab.scan);
  const selected = search.id ?? null;

  const runTabScan = () => (tab.scan === "repositories" ? activity.start("repositories") : activity.startInventory());

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title="Discovery"
        subtitle={scan ? `${tab.label}: last scanned ${formatRelative(scan.finished_at ?? scan.started_at)}` : undefined}
        actions={
          <Button size="sm" onClick={() => void runTabScan()} disabled={running}>
            <RefreshCw className={running ? "animate-spin" : ""} aria-hidden />
            {running ? progressLabel(activity.progress[tab.scan]) : tab.scan === "repositories" ? "Rescan folders" : "Rescan software"}
          </Button>
        }
      />
      <div className="flex shrink-0 flex-col gap-2.5 border-b border-line px-5 pt-2.5 pb-2.5">
        <div className="flex items-center gap-1" role="tablist">
          {TABS.map((t) => (
            <button
              key={t.id}
              role="tab"
              aria-selected={t.id === tab.id}
              onClick={() => void nav({ search: { tab: t.id, q: search.q } })}
              className={cn(
                "h-7 rounded-md px-2.5 text-[12.5px] transition-colors",
                t.id === tab.id ? "bg-hover text-fg" : "text-muted hover:text-fg",
              )}
            >
              {t.label}
            </button>
          ))}
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <label className="flex h-7 w-72 items-center gap-1.5 rounded-md border border-line bg-surface px-2 focus-within:border-accent/60">
            <Search className="size-3.5 text-faint" aria-hidden />
            <input
              value={text}
              onChange={(e) => setText(e.target.value)}
              placeholder={`Search ${tab.label.toLowerCase()}`}
              aria-label="Search"
              className="min-w-0 flex-1 bg-transparent text-[12.5px] text-fg outline-none placeholder:text-faint"
            />
            {text ? (
              <button aria-label="Clear search" onClick={() => setText("")} className="text-faint hover:text-fg">
                <X className="size-3.5" />
              </button>
            ) : null}
          </label>
          <div className="flex flex-wrap items-center gap-1">
            {filters.map((f) => (
              <button
                key={f.id}
                onClick={() => void nav({ search: (s) => ({ ...s, filter: f.id === "all" ? undefined : f.id }) })}
                className={cn(
                  "h-6 rounded-full border px-2.5 text-[11.5px] transition-colors",
                  f.id === filter.id ? "border-accent/50 bg-accent/10 text-fg" : "border-line text-muted hover:text-fg",
                )}
              >
                {f.label}
                {f.id !== "all" ? <span className="ml-1 text-faint">{rows.filter(f.test).length}</span> : null}
              </button>
            ))}
          </div>
          <span className="ml-auto text-[11.5px] text-faint tabular">
            {formatNumber(visible.length)} {visible.length === rows.length ? "" : `of ${formatNumber(rows.length)}`}
          </span>
        </div>
      </div>
      <div className="flex min-h-0 flex-1">
        <div className="min-w-0 flex-1 overflow-auto">
          {error ? (
            <StateView kind="error" title="Could not load results" description={errorMessage(error)} />
          ) : loading && rows.length === 0 ? (
            <StateView kind="loading" title="Loading" />
          ) : rows.length === 0 ? (
            <EmptyTab tab={tab.id} scanned={Boolean(scan)} query={q} running={running} roots={overview.data?.scan_roots ?? 0} onScan={() => void runTabScan()} />
          ) : visible.length === 0 ? (
            <StateView kind="empty" title="Nothing matches this filter" />
          ) : (
            <table className="w-full border-separate border-spacing-0 text-[12.5px]">
              <thead className="sticky top-0 z-[1] bg-bg">
                <tr>
                  {columns.map((c) => (
                    <th
                      key={c.id}
                      scope="col"
                      className={cn("border-b border-line px-3 py-1.5 text-left text-[11.5px] font-medium text-muted first:pl-5", c.className)}
                    >
                      {c.sort ? (
                        <button
                          className="inline-flex items-center gap-1 hover:text-fg"
                          onClick={() => setSort((s) => ({ col: c.id, desc: s.col === c.id ? !s.desc : false }))}
                        >
                          {c.header}
                          {sort.col === c.id ? sort.desc ? <ArrowDown className="size-3" /> : <ArrowUp className="size-3" /> : null}
                        </button>
                      ) : (
                        c.header
                      )}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {visible.map((e) => (
                  <tr
                    key={e.id}
                    onClick={() => void nav({ search: (s) => ({ ...s, id: s.id === e.id ? undefined : e.id }) })}
                    aria-selected={selected === e.id}
                    className={cn("cursor-pointer", selected === e.id ? "bg-accent/10" : "hover:bg-hover/50")}
                  >
                    {columns.map((c) => (
                      <td key={c.id} className={cn("max-w-[320px] truncate border-b border-line/60 px-3 py-1.5 first:pl-5", c.className)}>
                        {c.cell(e)}
                      </td>
                    ))}
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
        {selected ? (
          <EntityDetailPanel
            id={selected}
            onClose={() => void nav({ search: (s) => ({ ...s, id: undefined }) })}
            onNavigate={(id, kind) =>
              AI_KINDS.includes(kind)
                ? void nav({ to: "/ai", search: { tab: "overview", id } })
                : void nav({ search: { tab: tabForKind(kind), id } })
            }
            actions={
              <Button
                size="sm"
                variant="ghost"
                onClick={() => void nav({ to: "/graph", search: { view: "focus", focus: selected, depth: 1, sel: selected } })}
              >
                <Network aria-hidden /> Open in graph
              </Button>
            }
            extraActions={
              <Button size="sm" variant="ghost" onClick={() => void nav({ to: "/impact", search: { tab: "impact", id: selected } })}>
                What if removed?
              </Button>
            }
            extra={(entity) =>
              entity.kind === "runtime" ? (
                <section className="flex flex-col gap-2">
                  <h3 className="text-[11px] font-medium uppercase tracking-[0.07em] text-faint">What if this were removed?</h3>
                  <RuntimeImpact id={entity.id} onSelectProject={(id) => void nav({ search: { tab: "repositories", id } })} />
                </section>
              ) : null
            }
          />
        ) : null}
      </div>
    </div>
  );
}

function EmptyTab({ tab, scanned, query, running, roots, onScan }: { tab: DiscoveryTab; scanned: boolean; query: string; running: boolean; roots: number; onScan: () => void }) {
  const nav = useNavigate();
  if (query) return <StateView kind="empty" title={`No matches for “${query}”`} />;
  if (running) return <StateView kind="loading" title="Scan in progress" description="Results appear here when it finishes." />;
  if (tab === "repositories" && roots === 0)
    return (
      <StateView
        kind="empty"
        title="No project folders chosen"
        description="AGASTHY only looks for Git repositories inside folders you choose. It never scans your whole disk."
        action={
          <Button variant="primary" onClick={() => void nav({ to: "/settings" })}>
            <FolderPlus aria-hidden /> Choose folders
          </Button>
        }
      />
    );
  if (!scanned)
    return (
      <StateView
        kind="empty"
        title="Not scanned yet"
        action={
          <Button variant="primary" onClick={onScan}>
            Scan now
          </Button>
        }
      />
    );
  const what: Record<DiscoveryTab, string> = {
    repositories: `No Git repositories were found in your ${roots} folder${roots === 1 ? "" : "s"}`,
    applications: "No applications were found",
    tools: "None of the catalogued tools were found on PATH",
    packages: "No packages were reported by the package managers that could be read",
    managers: "No package managers were checked",
    dependencies: "None of the scanned repositories declare dependencies in a supported manifest",
  };
  return <StateView kind="empty" title={what[tab]} description="See Settings → Scan history for any issues reported during the scan." />;
}
