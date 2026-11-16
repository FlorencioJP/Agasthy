import type { Entity, FindingAttrs } from "@agasthy/shared-types";
import { Badge, Button, StateView, cn } from "@agasthy/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Globe, RefreshCw } from "lucide-react";
import { useState } from "react";
import { SEVERITY } from "../components/finding-attributes";
import { PageHeader, Panel } from "../components/page";
import { progressLabel, useScanActivity } from "../components/scan-activity";
import { formatNumber, formatRelative } from "../lib/format";
import { api, errorMessage } from "../lib/ipc";
import { useEntities, useOverview } from "../lib/queries";
import { EntityDetailPanel } from "./entity-detail";

const A = (e: Entity) => e.attributes as unknown as FindingAttrs;
const ORDER = ["critical", "high", "medium", "low", "unknown", "info"];

export function SecurityPage() {
  const qc = useQueryClient();
  const settings = useQuery({ queryKey: ["security-settings"], queryFn: api.securitySettings });
  const findings = useEntities("finding", "");
  const activity = useScanActivity();
  const overview = useOverview();
  const [category, setCategory] = useState("all");
  const [selected, setSelected] = useState<string | null>(null);
  const [showPreview, setShowPreview] = useState(false);
  const running = activity.running.has("security");
  const scan = overview.data?.latest_scans.find((s) => s.kind === "security");

  const toggle = async (on: boolean) => {
    await api.setOsvEnabled(on);
    await qc.invalidateQueries({ queryKey: ["security-settings"] });
  };
  const run = async () => {
    await activity.start("security");
    await qc.invalidateQueries({ queryKey: ["security-settings"] });
  };

  const rows = (findings.data ?? [])
    .filter((f) => category === "all" || A(f).category === category)
    .sort((a, b) => ORDER.indexOf(A(a).severity) - ORDER.indexOf(A(b).severity));
  const count = (c: string) => (findings.data ?? []).filter((f) => A(f).category === c).length;
  const s = settings.data;
  const report = s?.last_report;

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title="Security"
        subtitle={scan ? `checked ${formatRelative(scan.finished_at ?? scan.started_at)}` : "not checked yet"}
        actions={
          <Button size="sm" onClick={() => void run()} disabled={running}>
            <RefreshCw className={running ? "animate-spin" : ""} aria-hidden /> {running ? progressLabel(activity.progress.security) : "Check now"}
          </Button>
        }
      />
      <div className="flex min-h-0 flex-1">
        <div className="flex min-w-0 flex-1 flex-col gap-4 overflow-y-auto p-5">
          <Panel title="Vulnerability advisories (OSV.dev)">
            {settings.isLoading ? (
              <StateView kind="loading" title="Loading" compact />
            ) : settings.error ? (
              <StateView kind="error" title="Could not read settings" description={errorMessage(settings.error)} compact />
            ) : s ? (
              <div className="flex flex-col gap-2">
                <label className="flex items-start gap-2 text-[12.5px] text-fg">
                  <input type="checkbox" checked={s.osv_enabled} onChange={(e) => void toggle(e.target.checked)} className="mt-0.5 accent-[var(--accent)]" aria-label="Check advisories online with OSV.dev" />
                  <span>
                    <span className="font-medium">Check advisories online with OSV.dev</span> <Badge tone={s.osv_enabled ? "accent" : "neutral"}>{s.osv_enabled ? "on" : "off"}</Badge>
                    <span className="mt-0.5 block text-[12px] leading-5 text-muted">
                      <Globe className="mr-1 inline size-3.5" aria-hidden />
                      When on, each check sends {formatNumber(s.preview_total)} package name / ecosystem / version combinations to api.osv.dev over HTTPS. Nothing else is
                      sent: no paths, project names, hostnames or credentials. Off by default.
                    </span>
                  </span>
                </label>
                <button className="self-start text-[12px] text-accent hover:underline" onClick={() => setShowPreview((v) => !v)}>
                  {showPreview ? "Hide" : "Show"} exactly what would be sent
                </button>
                {showPreview ? (
                  <pre className="selectable max-h-56 overflow-auto rounded bg-raised p-2 font-mono text-[11px] text-muted">
                    {s.preview.map((q) => `${q.ecosystem}  ${q.name}@${q.version}`).join("\n")}
                    {s.preview_total > s.preview.length ? `\n…and ${s.preview_total - s.preview.length} more` : ""}
                  </pre>
                ) : null}
                <p className="text-[11.5px] leading-5 text-faint">
                  Covered: npm, PyPI, crates.io and Go packages that AGASTHY can identify with an exact version (global installs, and project dependencies pinned by a lockfile).
                  Not covered: Homebrew (outdated status is reported instead), applications, and anything without a known version.
                </p>
              </div>
            ) : null}
          </Panel>
          {report ? (
            <Panel title="Last check">
              <div className="flex flex-wrap gap-1.5 text-[12px]">
                <Badge tone={report.vulnerabilities ? "danger" : "neutral"}>{report.vulnerabilities} advisories</Badge>
                <Badge>{report.outdated} outdated</Badge>
                <Badge>{report.unknown_origin} unknown origin</Badge>
                <Badge tone={report.osv_enabled ? "accent" : "neutral"}>{report.osv_enabled ? `${report.sent_to_osv} versions checked online` : "online check off"}</Badge>
              </div>
              <p className="mt-2 text-[11.5px] leading-5 text-faint">
                No advisory found does not mean secure: it means no known advisory matched the versions AGASTHY could identify.
              </p>
            </Panel>
          ) : null}
          <Panel
            title="Findings"
            bodyClassName="p-0"
            actions={
              <div className="flex gap-1">
                {[
                  ["all", "All"],
                  ["vulnerability", `Advisories ${count("vulnerability")}`],
                  ["outdated", `Outdated ${count("outdated")}`],
                  ["provenance", `Unknown origin ${count("provenance")}`],
                ].map(([id, label]) => (
                  <button
                    key={id}
                    onClick={() => setCategory(id)}
                    className={cn("h-6 rounded-full border px-2.5 text-[11.5px]", category === id ? "border-accent/50 bg-accent/10 text-fg" : "border-line text-muted")}
                  >
                    {label}
                  </button>
                ))}
              </div>
            }
          >
            {findings.isLoading ? (
              <StateView kind="loading" title="Loading" compact />
            ) : findings.error ? (
              <StateView kind="error" title="Could not load findings" description={errorMessage(findings.error)} compact />
            ) : !scan ? (
              <StateView kind="empty" title="Not checked yet" description="Run a check. Without OSV.dev it uses only local information." compact />
            ) : rows.length === 0 ? (
              <StateView kind="empty" title="No findings in this category" compact />
            ) : (
              <ul className="divide-y divide-line">
                {rows.map((f) => (
                  <li key={f.id}>
                    <button className={cn("flex w-full items-center justify-between gap-3 px-3.5 py-2 text-left", selected === f.id ? "bg-accent/10" : "hover:bg-hover/50")} onClick={() => setSelected(f.id === selected ? null : f.id)}>
                      <div className="min-w-0">
                        <div className="truncate text-[12.5px] text-fg">{f.name}</div>
                        <div className="truncate text-[11.5px] text-faint">{A(f).remediation}</div>
                      </div>
                      <Badge tone={SEVERITY[A(f).severity].tone}>{SEVERITY[A(f).severity].label}</Badge>
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </Panel>
        </div>
        {selected ? <EntityDetailPanel id={selected} onClose={() => setSelected(null)} onNavigate={(id) => setSelected(id)} /> : null}
      </div>
    </div>
  );
}
