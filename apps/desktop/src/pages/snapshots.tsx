import type { SnapshotDiff, SnapshotItem } from "@agasthy/shared-types";
import { Badge, Button, StateView } from "@agasthy/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Camera, Copy, FolderOpen } from "lucide-react";
import { useState } from "react";
import { PageHeader, Panel } from "../components/page";
import { formatDateTime, shortenHome } from "../lib/format";
import { api, errorMessage, revealInFileManager } from "../lib/ipc";

const SECTION: Record<string, string> = {
  application: "Applications",
  tool: "CLI tools",
  runtime: "Runtimes",
  package: "Packages",
  repository: "Repositories",
  ai_tool: "AI tools",
  mcp_server: "MCP servers",
  env_var: "Environment variable names",
};

function group(items: SnapshotItem[]) {
  const m = new Map<string, SnapshotItem[]>();
  for (const i of items) m.set(i.section, [...(m.get(i.section) ?? []), i]);
  return [...m.entries()];
}

export function SnapshotsPage() {
  const qc = useQueryClient();
  const list = useQuery({ queryKey: ["snapshots"], queryFn: api.snapshotsList });
  const [name, setName] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<{ kind: "diff" | "plan"; id: string; against?: string | null } | null>(null);

  const create = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.snapshotCreate(name.trim() || `Snapshot ${new Date().toLocaleString()}`);
      setName("");
      await qc.invalidateQueries({ queryKey: ["snapshots"] });
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader title="Snapshots" subtitle="A record of what is installed and where your repositories are. Metadata only; not a backup." />
      <div className="flex max-w-5xl flex-col gap-4 overflow-y-auto p-5">
        <Panel title="Take a snapshot">
          <div className="flex gap-2">
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="Name, e.g. Before upgrading Python"
              aria-label="Snapshot name"
              className="h-8 flex-1 rounded-md border border-line bg-surface px-2 text-[12.5px] text-fg placeholder:text-faint"
            />
            <Button variant="primary" onClick={() => void create()} disabled={busy}>
              <Camera aria-hidden /> {busy ? "Saving…" : "Take snapshot"}
            </Button>
          </div>
          <p className="mt-2 text-[11.5px] leading-5 text-faint">
            Records applications, tools, runtimes, packages, repositories (commit, branch, remote), AI tools, MCP servers and the names of environment
            variables, from AGASTHY's latest scans. No file contents, credentials or environment values. Run the scans first for a complete picture.
          </p>
          {error ? <div className="mt-2 text-[12px] text-danger">{error}</div> : null}
        </Panel>
        <Panel title="Snapshots" bodyClassName="p-0">
          {list.isLoading ? (
            <StateView kind="loading" title="Loading" compact />
          ) : list.error ? (
            <StateView kind="error" title="Could not read snapshots" description={errorMessage(list.error)} compact />
          ) : !list.data?.length ? (
            <StateView kind="empty" title="No snapshots yet" compact />
          ) : (
            <ul className="divide-y divide-line">
              {list.data.map((s) => (
                <li key={s.id} className="flex items-center justify-between gap-3 px-3.5 py-2">
                  <div className="min-w-0">
                    <div className="text-[12.5px] text-fg">{s.name}</div>
                    <div className="truncate text-[11.5px] text-faint">
                      {formatDateTime(s.created_at)}
                      {s.hostname ? ` · ${s.hostname}` : ""} ·{" "}
                      {Object.entries(s.counts)
                        .map(([k, v]) => `${v} ${SECTION[k]?.toLowerCase() ?? k}`)
                        .join(", ")}
                    </div>
                  </div>
                  <div className="flex shrink-0 gap-1.5">
                    <Button size="sm" onClick={() => setView({ kind: "diff", id: s.id, against: null })}>
                      Compare with now
                    </Button>
                    {list.data.length > 1 ? (
                      <select
                        aria-label="Compare with another snapshot"
                        className="h-7 rounded-md border border-line bg-surface px-1.5 text-[12px] text-fg"
                        value=""
                        onChange={(e) => e.target.value && setView({ kind: "diff", id: s.id, against: e.target.value })}
                      >
                        <option value="">Compare with…</option>
                        {list.data
                          .filter((o) => o.id !== s.id)
                          .map((o) => (
                            <option key={o.id} value={o.id}>
                              {o.name}
                            </option>
                          ))}
                      </select>
                    ) : null}
                    <Button size="sm" onClick={() => setView({ kind: "plan", id: s.id })}>
                      What's missing?
                    </Button>
                    <Button size="sm" variant="ghost" aria-label="Show file" onClick={() => void revealInFileManager(s.path)}>
                      <FolderOpen />
                    </Button>
                  </div>
                </li>
              ))}
            </ul>
          )}
        </Panel>
        {view?.kind === "diff" ? <DiffView from={view.id} to={view.against ?? null} /> : null}
        {view?.kind === "plan" ? <PlanView id={view.id} /> : null}
      </div>
    </div>
  );
}

function DiffView({ from, to }: { from: string; to: string | null }) {
  const q = useQuery({ queryKey: ["snapshot-diff", from, to], queryFn: () => api.snapshotCompare(from, to) });
  if (q.isLoading) return <StateView kind="loading" title="Comparing" compact />;
  if (q.error) return <StateView kind="error" title="Could not compare" description={errorMessage(q.error)} compact />;
  const d = q.data as SnapshotDiff;
  return (
    <Panel title={`Changes since the snapshot${to ? "" : " (compared with now)"}`}>
      <div className="mb-3 flex gap-1.5">
        <Badge tone="success">{d.added.length} added</Badge>
        <Badge tone="danger">{d.removed.length} removed</Badge>
        <Badge tone="info">{d.changed.length} changed</Badge>
      </div>
      {d.added.length + d.removed.length + d.changed.length === 0 ? <div className="text-[12.5px] text-muted">No differences.</div> : null}
      <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
        {(
          [
            ["Added", d.added],
            ["Removed", d.removed],
          ] as const
        ).map(([title, items]) => (
          <div key={title}>
            <div className="mb-1 text-[11px] font-medium uppercase tracking-[0.07em] text-faint">{title}</div>
            {group(items).map(([sec, list]) => (
              <div key={sec} className="mb-2">
                <div className="text-[11.5px] text-muted">{SECTION[sec] ?? sec}</div>
                <ul className="text-[12px] text-fg">
                  {list.slice(0, 40).map((i) => (
                    <li key={i.key} className="truncate">
                      {i.name} <span className="text-faint">{i.version ?? ""}</span>
                    </li>
                  ))}
                  {list.length > 40 ? <li className="text-faint">…and {list.length - 40} more</li> : null}
                </ul>
              </div>
            ))}
          </div>
        ))}
        <div>
          <div className="mb-1 text-[11px] font-medium uppercase tracking-[0.07em] text-faint">Changed</div>
          <ul className="text-[12px] text-fg">
            {d.changed.slice(0, 60).map((c) => (
              <li key={c.section + c.key} className="truncate">
                {c.name}: <span className="text-faint">{c.before ?? "—"}</span> → {c.after ?? "—"}
              </li>
            ))}
          </ul>
        </div>
      </div>
    </Panel>
  );
}

function PlanView({ id }: { id: string }) {
  const q = useQuery({ queryKey: ["snapshot-plan", id], queryFn: () => api.snapshotReconstruction(id) });
  const [copied, setCopied] = useState(false);
  if (q.isLoading) return <StateView kind="loading" title="Comparing with now" compact />;
  if (q.error) return <StateView kind="error" title="Could not build the plan" description={errorMessage(q.error)} compact />;
  const p = q.data!;
  const script = p.steps
    .filter((s) => s.command)
    .map((s) => s.command)
    .join("\n");
  return (
    <Panel
      title="Missing since the snapshot"
      actions={
        script ? (
          <Button
            size="sm"
            variant="ghost"
            onClick={() =>
              void navigator.clipboard.writeText(script).then(() => {
                setCopied(true);
                setTimeout(() => setCopied(false), 1500);
              })
            }
          >
            <Copy aria-hidden /> {copied ? "Copied" : "Copy commands"}
          </Button>
        ) : null
      }
    >
      <p className="mb-2 text-[12px] leading-5 text-muted">
        {p.steps.length === 0
          ? "Everything in the snapshot is still present."
          : `${p.supported} item(s) have a command you can run to bring them back; ${p.unsupported} cannot be restored automatically. AGASTHY does not run these commands. Review them first.`}
      </p>
      <ul className="divide-y divide-line">
        {p.steps.map((s) => (
          <li key={s.item.section + s.item.key} className="py-1.5">
            <div className="text-[12.5px] text-fg">
              {s.item.name} <span className="text-faint">{SECTION[s.item.section] ?? s.item.section}</span>
            </div>
            {s.command ? <code className="selectable block break-all font-mono text-[11.5px] text-accent">{shortenHome(s.command)}</code> : null}
            {s.note ? <div className="text-[11.5px] text-faint">{s.note}</div> : null}
          </li>
        ))}
      </ul>
    </Panel>
  );
}
