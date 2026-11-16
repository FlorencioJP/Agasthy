import type { ActionRequest, CleanupCandidate, CleanupCategory, DiskSample, SafetyLevel } from "@agasthy/shared-types";
import { Badge, Button, Meter, StateView, cn, type BadgeProps } from "@agasthy/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { HardDrive, RefreshCw, RotateCcw, Trash2 } from "lucide-react";
import { useState } from "react";
import { ActionDialog } from "../components/action-dialog";
import { PageHeader, Panel } from "../components/page";
import { formatBytes, formatDateTime, formatRelative, shortenHome, value } from "../lib/format";
import { api, errorMessage } from "../lib/ipc";
import { useTelemetry } from "../lib/queries";

const CATEGORY: Record<CleanupCategory, string> = {
  node_modules: "node_modules",
  rust_target: "Rust target/",
  python_venv: "Python virtualenvs",
  build_output: "Build output",
  package_cache: "Package caches",
  xcode_derived_data: "Xcode DerivedData",
  developer_cache: "Developer caches",
  model_cache: "Model caches",
  duplicate_clone: "Duplicate clones",
};

const SAFETY: Record<SafetyLevel, { label: string; tone: BadgeProps["tone"] }> = {
  safe: { label: "safe", tone: "success" },
  caution: { label: "caution", tone: "warning" },
  review_only: { label: "review only", tone: "neutral" },
};

const size = (c: CleanupCandidate) => value(c.size_bytes) ?? 0;

export function StoragePage() {
  const qc = useQueryClient();
  const cached = useQuery({ queryKey: ["disk-analysis", "cached"], queryFn: api.cachedDiskAnalysis });
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [category, setCategory] = useState<CleanupCategory | "all">("all");
  const [action, setAction] = useState<ActionRequest | null>(null);
  const quarantine = useQuery({ queryKey: ["quarantine"], queryFn: api.quarantineList });
  const telemetry = useTelemetry();
  const analysis = cached.data ?? null;

  const analyse = async () => {
    setRunning(true);
    setError(null);
    try {
      const a = await api.diskAnalysis();
      qc.setQueryData(["disk-analysis", "cached"], a);
      setSelected(new Set());
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setRunning(false);
    }
  };

  const rows = (analysis?.candidates ?? []).filter((c) => category === "all" || c.category === category);
  const selectedBytes = (analysis?.candidates ?? []).filter((c) => selected.has(c.path)).reduce((s, c) => s + size(c), 0);
  const toggle = (p: string) =>
    setSelected((s) => {
      const n = new Set(s);
      if (n.has(p)) n.delete(p);
      else n.add(p);
      return n;
    });
  const disks: DiskSample[] = (telemetry.data && value(telemetry.data.disks)) || [];
  const visibleDisks = disks.filter((d) => !d.system_volume);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title="Storage & Cleanup"
        subtitle={analysis ? `analysed ${formatRelative(analysis.analysed_at)}` : "not analysed yet"}
        actions={
          <Button size="sm" onClick={() => void analyse()} disabled={running}>
            <RefreshCw className={running ? "animate-spin" : ""} aria-hidden /> {running ? "Measuring…" : analysis ? "Analyse again" : "Analyse disk"}
          </Button>
        }
      />
      <div className="flex flex-col gap-4 overflow-y-auto p-5">
        {visibleDisks.length ? (
          <Panel title="Volumes" bodyClassName="grid grid-cols-1 gap-3 md:grid-cols-2">
            {visibleDisks.map((d) => (
              <div key={d.mount_point} className="flex flex-col gap-1.5">
                <div className="flex justify-between text-[12px]">
                  <span className="flex items-center gap-1.5 text-fg">
                    <HardDrive className="size-3.5 text-faint" aria-hidden /> {d.mount_point}
                  </span>
                  <span className="tabular text-muted">
                    {formatBytes(d.available_bytes)} free of {formatBytes(d.total_bytes, 0)}
                  </span>
                </div>
                <Meter value={d.total_bytes ? (d.total_bytes - d.available_bytes) / d.total_bytes : null} label={`Disk ${d.mount_point}`} />
              </div>
            ))}
          </Panel>
        ) : null}
        {error ? <StateView kind="error" title="Analysis failed" description={error} compact /> : null}
        {!analysis && !running ? (
          <StateView
            kind="empty"
            title="Find space you can safely reclaim"
            description="AGASTHY measures dependency folders and build output inside your project folders, and a fixed list of developer caches. It changes nothing: you choose what to clean, review a dry run, and items go to a quarantine you can restore from."
            action={
              <Button variant="primary" onClick={() => void analyse()}>
                Analyse disk
              </Button>
            }
          />
        ) : null}
        {running && !analysis ? <StateView kind="loading" title="Measuring folders (this can take a minute)" /> : null}
        {analysis ? (
          <>
            <Panel title="Where the space is" actions={<span className="text-[11px] text-faint">{formatBytes(analysis.eligible_bytes)} can be cleaned</span>}>
              <div className="flex flex-wrap gap-1.5">
                <button onClick={() => setCategory("all")} className={cn("h-7 rounded-full border px-3 text-[12px]", category === "all" ? "border-accent/50 bg-accent/10 text-fg" : "border-line text-muted")}>
                  All
                </button>
                {analysis.totals.map((t) => (
                  <button
                    key={t.category}
                    onClick={() => setCategory(t.category)}
                    className={cn("h-7 rounded-full border px-3 text-[12px]", category === t.category ? "border-accent/50 bg-accent/10 text-fg" : "border-line text-muted hover:text-fg")}
                  >
                    {CATEGORY[t.category]} <span className="tabular text-faint">{formatBytes(t.bytes)}</span>
                  </button>
                ))}
              </div>
            </Panel>
            <Panel
              title="Cleanup candidates"
              bodyClassName="p-0"
              actions={
                <Button size="sm" variant="primary" disabled={selected.size === 0} onClick={() => setAction({ type: "quarantine_paths", paths: [...selected] })}>
                  Review {selected.size ? `${selected.size} (${formatBytes(selectedBytes)})` : ""}…
                </Button>
              }
            >
              {rows.length === 0 ? (
                <StateView kind="empty" title="Nothing found in this category" compact />
              ) : (
                <table className="w-full text-[12.5px]">
                  <tbody>
                    {rows.map((c) => (
                      <tr key={c.id} className="border-t border-line first:border-t-0 align-top">
                        <td className="w-8 py-2 pl-3.5">
                          <input
                            type="checkbox"
                            aria-label={`Select ${c.path}`}
                            disabled={!c.eligible}
                            checked={selected.has(c.path)}
                            onChange={() => toggle(c.path)}
                            className="mt-0.5 accent-[var(--accent)]"
                          />
                        </td>
                        <td className="py-2 pr-3">
                          <div className="selectable break-all font-mono text-[11.5px] text-fg">{shortenHome(c.path)}</div>
                          <div className="text-[11.5px] text-muted">
                            {c.reason}
                            {c.regenerate ? <span className="text-faint"> · {c.regenerate}</span> : null}
                          </div>
                          {!c.eligible && c.blocked_by ? <div className="text-[11.5px] text-warning">Not offered: {c.blocked_by}</div> : null}
                        </td>
                        <td className="w-24 py-2 pr-3">
                          <Badge tone={SAFETY[c.safety].tone}>{SAFETY[c.safety].label}</Badge>
                        </td>
                        <td className="w-28 py-2 pr-3 text-[11.5px] text-faint">{c.last_modified ? formatRelative(c.last_modified) : ""}</td>
                        <td className="tabular w-24 py-2 pr-3.5 text-right text-fg">{c.size_bytes.status === "available" ? formatBytes(c.size_bytes.value) : "—"}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
              <p className="border-t border-line px-3.5 py-2 text-[11.5px] leading-5 text-faint">
                "Last modified" is shown for context only: an old folder is not assumed to be unused. Repositories and files tracked by Git are never offered. Cleaning disk space does not free memory.
              </p>
            </Panel>
            <Panel title="Docker">
              {analysis.docker.status === "available" ? (
                <div className="flex flex-col gap-2">
                  <table className="w-full text-[12px]">
                    <tbody>
                      {analysis.docker.value.rows.map((r) => (
                        <tr key={r.kind} className="border-t border-line first:border-t-0">
                          <td className="py-1 text-fg">{r.kind}</td>
                          <td className="py-1 text-muted">{r.total} total · {r.active} active</td>
                          <td className="py-1 text-right text-muted">{r.size}</td>
                          <td className="py-1 text-right text-faint">reclaimable {r.reclaimable}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                  <div className="flex gap-1.5">
                    <Button size="sm" onClick={() => setAction({ type: "docker_prune", target: "build_cache" })}>
                      Prune build cache…
                    </Button>
                    <Button size="sm" onClick={() => setAction({ type: "docker_prune", target: "dangling_images" })}>
                      Remove {analysis.docker.value.dangling_images} dangling image(s)…
                    </Button>
                  </div>
                  <p className="text-[11.5px] text-faint">Volumes hold data and are never removed by AGASTHY.</p>
                </div>
              ) : (
                <StateView kind="unsupported" title="Docker unavailable" description={analysis.docker.status === "unavailable" ? analysis.docker.reason : undefined} compact />
              )}
            </Panel>
          </>
        ) : null}
        <Panel title="Quarantine" bodyClassName="p-0" actions={<span className="text-[11px] text-faint">items moved here can be restored</span>}>
          {quarantine.isLoading ? (
            <StateView kind="loading" title="Loading" compact />
          ) : quarantine.error ? (
            <StateView kind="error" title="Could not read the quarantine" description={errorMessage(quarantine.error)} compact />
          ) : !quarantine.data?.length ? (
            <StateView kind="empty" title="Quarantine is empty" compact />
          ) : (
            <ul className="divide-y divide-line">
              {quarantine.data.map((op) => (
                <li key={op.id} className="flex items-center justify-between gap-3 px-3.5 py-2">
                  <div className="min-w-0">
                    <div className="text-[12.5px] text-fg">
                      {op.items.length} item{op.items.length === 1 ? "" : "s"} · {formatBytes(op.total_bytes)} <Badge>{op.state.replaceAll("_", " ")}</Badge>
                    </div>
                    <div className="truncate text-[11.5px] text-faint">
                      {formatDateTime(op.created_at)} · {op.items.map((i) => shortenHome(i.original_path)).join(", ")}
                    </div>
                  </div>
                  {op.state === "quarantined" || op.state === "partially_restored" ? (
                    <div className="flex shrink-0 gap-1.5">
                      <Button size="sm" onClick={() => setAction({ type: "restore_quarantine", op_id: op.id })}>
                        <RotateCcw aria-hidden /> Restore…
                      </Button>
                      <Button size="sm" variant="danger" onClick={() => setAction({ type: "purge_quarantine", op_id: op.id })}>
                        <Trash2 aria-hidden /> Delete permanently…
                      </Button>
                    </div>
                  ) : null}
                </li>
              ))}
            </ul>
          )}
        </Panel>
      </div>
      {action ? (
        <ActionDialog
          request={action}
          onClose={(changed) => {
            setAction(null);
            if (changed) {
              setSelected(new Set());
              void qc.invalidateQueries({ queryKey: ["quarantine"] });
              void qc.invalidateQueries({ queryKey: ["disk-analysis"] });
            }
          }}
        />
      ) : null}
    </div>
  );
}
