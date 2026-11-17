import type { ScanKind } from "@agasthy/shared-types";
import { Badge, Button, StateView } from "@agasthy/ui";
import { FolderPlus, RefreshCw, Trash2 } from "lucide-react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Fragment, useState } from "react";
import { AiModelPanel } from "../components/ai-model-panel";
import { ReportDialog } from "../components/report-dialog";
import { Kv, PageHeader, Panel } from "../components/page";
import { SCAN_LABELS, progressLabel, useScanActivity } from "../components/scan-activity";
import { formatDateTime, formatNumber, formatRelative, shortenHome } from "../lib/format";
import { api, errorMessage, pickFolder, revealInFileManager } from "../lib/ipc";
import { autoCheckEnabled, checkForUpdate, installUpdate, setAutoCheck, updatesSupported, useUpdateState } from "../lib/updates";
import { useAppInfo, useAudit, useInvalidateData, useScanRoots, useScans } from "../lib/queries";

export function SettingsPage() {
  return (
    <>
      <PageHeader title="Settings" />
      <div className="flex max-w-5xl flex-col gap-4 p-5">
        <FoldersPanel />
        <ScansPanel />
        <AiModelPanel />
        <HistoryPanel />
        <AuditPanel />
        <AboutPanel />
      </div>
    </>
  );
}

function FoldersPanel() {
  const roots = useScanRoots();
  const invalidate = useInvalidateData();
  const activity = useScanActivity();
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const add = async () => {
    setError(null);
    const path = await pickFolder().catch((e) => {
      setError(errorMessage(e));
      return null;
    });
    if (!path) return;
    setBusy(true);
    try {
      await api.addScanRoot(path);
      await invalidate();
    } catch (e) {
      setError(errorMessage(e));
    } finally {
      setBusy(false);
    }
  };

  const running = activity.running.has("repositories");
  return (
    <Panel
      title="Project folders"
      actions={
        <div className="flex gap-1.5">
          <Button size="sm" variant="ghost" onClick={() => void activity.start("repositories")} disabled={running || !roots.data?.some((r) => r.enabled)}>
            <RefreshCw className={running ? "animate-spin" : ""} aria-hidden />
            {running ? progressLabel(activity.progress.repositories) : "Scan folders"}
          </Button>
          <Button size="sm" onClick={() => void add()} disabled={busy}>
            <FolderPlus aria-hidden /> Add folder…
          </Button>
        </div>
      }
    >
      <p className="mb-3 text-[12px] leading-5 text-muted">
        AGASTHY looks for Git repositories only inside these folders, up to 6 levels deep. It does not follow symbolic links out of a folder and skips
        dependency and build directories such as <code className="font-mono">node_modules</code> and <code className="font-mono">target</code>. System
        locations and your Library folder cannot be added.
      </p>
      {error ? <div className="mb-2 rounded-md bg-danger/10 px-2.5 py-1.5 text-[12px] text-danger">{error}</div> : null}
      {activity.lastError.repositories ? (
        <div className="mb-2 rounded-md bg-danger/10 px-2.5 py-1.5 text-[12px] text-danger">{activity.lastError.repositories}</div>
      ) : null}
      {roots.isLoading ? (
        <StateView kind="loading" title="Loading folders" compact />
      ) : roots.error ? (
        <StateView kind="error" title="Could not load folders" description={errorMessage(roots.error)} compact />
      ) : !roots.data?.length ? (
        <StateView kind="empty" title="No folders yet" description="Add the folders where you keep code, for example ~/code or ~/projects." compact />
      ) : (
        <ul className="divide-y divide-line rounded-md border border-line">
          {roots.data.map((r) => (
            <li key={r.id} className="flex items-center justify-between gap-3 px-3 py-2">
              <div className="min-w-0">
                <div className="selectable truncate font-mono text-[12px] text-fg">{shortenHome(r.path)}</div>
                <div className="text-[11px] text-faint">added {formatRelative(r.added_at)} · depth {r.max_depth}</div>
              </div>
              <div className="flex shrink-0 items-center gap-1.5">
                <label className="flex items-center gap-1.5 text-[12px] text-muted">
                  <input
                    type="checkbox"
                    checked={r.enabled}
                    onChange={(e) => {
                      api
                        .setScanRootEnabled(r.id, e.target.checked)
                        .then(invalidate)
                        .catch((err) => setError(errorMessage(err)));
                    }}
                    className="accent-[var(--accent)]"
                  />
                  Enabled
                </label>
                <Button
                  size="icon"
                  variant="ghost"
                  aria-label={`Remove ${r.path}`}
                  title="Stop scanning this folder (nothing on disk is touched)"
                  onClick={() => {
                    api
                      .removeScanRoot(r.id)
                      .then(invalidate)
                      .catch((err) => setError(errorMessage(err)));
                  }}
                >
                  <Trash2 />
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </Panel>
  );
}

const SCAN_DESCRIPTIONS: Record<ScanKind, string> = {
  repositories: "Git repositories inside your project folders: branch, commit, working-tree status, remotes, languages, size.",
  software: "Application bundles and a catalogue of developer tools on PATH, with versions from `--version`.",
  packages: "Read-only inventories from Homebrew, npm, pnpm, uv, pip and cargo. Homebrew can take a minute.",
  security: "Outdated Homebrew packages (local data), tools of unknown origin, and, only if you enable it on the Security page, OSV.dev advisories.",
  ai: "AI assistants, providers, MCP servers, local models and AI instruction files in fixed home locations and your scanned repositories. Credential files are checked for existence only.",
};

function ScansPanel() {
  const activity = useScanActivity();
  const qc = useQueryClient();
  const schedule = useQuery({ queryKey: ["scan-schedule"], queryFn: api.scanSchedule });
  const toggle = async (on: boolean) => {
    await api.setAutoRefresh(on);
    await qc.invalidateQueries({ queryKey: ["scan-schedule"] });
  };
  return (
    <Panel title="Scans">
      {schedule.data ? (
        <label className="mb-3 flex items-start gap-2.5 rounded-md border border-line p-3 text-[12.5px]">
          <input type="checkbox" className="mt-0.5" checked={schedule.data.auto_refresh} onChange={(e) => void toggle(e.target.checked)} />
          <span>
            <span className="font-medium text-fg">Keep data fresh in the background</span>
            <span className="block text-[11.5px] text-muted">
              While AGASTHY is open, packages, applications and tools, repositories and the AI ecosystem are scanned again when their data is older than{" "}
              {schedule.data.refresh_after_hours} hours, but only areas you have scanned at least once. Security is never refreshed automatically. Background scans are
              read-only and appear in the audit log as “scheduler”.
            </span>
          </span>
        </label>
      ) : null}
      <ul className="divide-y divide-line">
        {(["repositories", "software", "packages", "ai", "security"] as ScanKind[]).map((k) => {
          const running = activity.running.has(k);
          return (
            <li key={k} className="flex items-center justify-between gap-4 py-2 first:pt-0 last:pb-0">
              <div className="min-w-0">
                <div className="text-[12.5px] text-fg">{SCAN_LABELS[k]}</div>
                <div className="text-[11.5px] text-faint">{SCAN_DESCRIPTIONS[k]}</div>
                {activity.lastError[k] ? <div className="text-[11.5px] text-danger">{activity.lastError[k]}</div> : null}
              </div>
              <Button size="sm" disabled={running} onClick={() => void activity.start(k)}>
                <RefreshCw className={running ? "animate-spin" : ""} aria-hidden />
                {running ? progressLabel(activity.progress[k]) : "Run"}
              </Button>
            </li>
          );
        })}
      </ul>
    </Panel>
  );
}

function HistoryPanel() {
  const scans = useScans(25);
  const [open, setOpen] = useState<number | null>(null);
  return (
    <Panel title="Scan history" bodyClassName="p-0">
      {scans.isLoading ? (
        <StateView kind="loading" title="Loading" compact />
      ) : scans.error ? (
        <StateView kind="error" title="Could not load history" description={errorMessage(scans.error)} compact />
      ) : !scans.data?.length ? (
        <StateView kind="empty" title="No scans yet" compact />
      ) : (
        <table className="w-full text-[12px]">
          <thead>
            <tr className="text-left text-[11px] text-muted">
              <th className="px-3.5 py-1.5 font-medium">Scan</th>
              <th className="px-3 py-1.5 font-medium">Status</th>
              <th className="px-3 py-1.5 text-right font-medium">Found</th>
              <th className="px-3 py-1.5 text-right font-medium">Removed</th>
              <th className="px-3 py-1.5 font-medium">Issues</th>
              <th className="px-3 py-1.5 font-medium">Started</th>
            </tr>
          </thead>
          <tbody>
            {scans.data.map((s) => (
              <Fragment key={s.id}>
                <tr className="border-t border-line">
                  <td className="px-3.5 py-1.5 text-fg">{SCAN_LABELS[s.kind]}</td>
                  <td className="px-3 py-1.5">
                    <Badge tone={s.status === "completed" ? "success" : s.status === "failed" ? "danger" : s.status === "running" ? "accent" : "warning"}>
                      {s.status.replaceAll("_", " ")}
                    </Badge>
                  </td>
                  <td className="tabular px-3 py-1.5 text-right">{formatNumber(s.entities_found)}</td>
                  <td className="tabular px-3 py-1.5 text-right text-muted" title="No longer present on the machine; removed from AGASTHY's database only">
                    {formatNumber(s.entities_removed)}
                  </td>
                  <td className="px-3 py-1.5">
                    {s.issues.length ? (
                      <button className="text-warning underline-offset-2 hover:underline" onClick={() => setOpen(open === s.id ? null : s.id)}>
                        {s.issues.length} issue{s.issues.length === 1 ? "" : "s"}
                      </button>
                    ) : (
                      <span className="text-faint">none</span>
                    )}
                  </td>
                  <td className="px-3 py-1.5 text-muted">{formatDateTime(s.started_at)}</td>
                </tr>
                {open === s.id ? (
                  <tr>
                    <td colSpan={6} className="bg-raised px-3.5 py-2">
                      <ul className="flex flex-col gap-1">
                        {s.issues.map((i, n) => (
                          <li key={n} className="text-[11.5px]">
                            <Badge tone={i.kind === "permission_denied" ? "warning" : "neutral"}>{i.kind.replaceAll("_", " ")}</Badge>{" "}
                            <span className="selectable font-mono text-muted">{shortenHome(i.subject)}</span> — <span className="text-fg">{i.message}</span>
                          </li>
                        ))}
                      </ul>
                    </td>
                  </tr>
                ) : null}
              </Fragment>
            ))}
          </tbody>
        </table>
      )}
    </Panel>
  );
}

function AuditPanel() {
  const audit = useAudit(50);
  return (
    <Panel title="Audit log" actions={<span className="text-[11px] text-faint">append-only · stored locally</span>} bodyClassName="p-0">
      {audit.isLoading ? (
        <StateView kind="loading" title="Loading" compact />
      ) : audit.error ? (
        <StateView kind="error" title="Could not load the audit log" description={errorMessage(audit.error)} compact />
      ) : !audit.data?.length ? (
        <StateView kind="empty" title="No actions recorded yet" compact />
      ) : (
        <table className="w-full text-[12px]">
          <tbody>
            {audit.data.map((a) => (
              <tr key={a.id} className="border-t border-line first:border-t-0">
                <td className="w-40 px-3.5 py-1.5 text-muted">{formatDateTime(a.at)}</td>
                <td className="px-3 py-1.5 font-mono text-[11.5px] text-fg">{a.action}</td>
                <td className="max-w-[280px] truncate px-3 py-1.5 text-muted" title={a.target}>
                  {shortenHome(a.target)}
                </td>
                <td className="px-3 py-1.5">
                  <Badge tone={a.outcome.startsWith("ok") || a.outcome === "started" ? "neutral" : "danger"}>{a.outcome}</Badge>
                </td>
                <td className="max-w-[320px] truncate px-3 py-1.5 text-faint" title={a.detail ?? ""}>
                  {a.detail}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
    </Panel>
  );
}

function AboutPanel() {
  const info = useAppInfo();
  return (
    <Panel title="About this build">
      {info.isLoading ? (
        <StateView kind="loading" title="Loading" compact />
      ) : info.error ? (
        <StateView kind="error" title="Could not read build information" description={errorMessage(info.error)} compact />
      ) : info.data ? (
        <Kv
          items={[
            ["Version", info.data.version],
            ["Platform", `${info.data.platform.os} ${info.data.platform.os_version ?? ""} · ${info.data.platform.arch}`],
            [
              "Platform support",
              {
                implemented: "adapters implemented and validated on this OS",
                unvalidated: "adapters implemented and unit-tested, not yet validated on real hardware for this OS",
                skeleton: "skeleton: most discovery is not implemented on this OS yet",
              }[info.data.platform.support_level],
            ],
            ["Database", <code className="selectable font-mono text-[11.5px]">{info.data.db_path}</code>],
            ["Schema version", info.data.schema_version],
            ["Updates", <UpdatesRow />],
            ["Problems", <ReportRow />],
            [
              "Diagnostics log",
              <span className="flex flex-wrap items-center gap-2">
                <span className="text-muted">Local only, never uploaded. Send it with a bug report.</span>
                <Button size="sm" variant="ghost" onClick={() => void revealInFileManager(`${info.data.data_dir}/logs/agasthy.log`)}>
                  Show in {{ macos: "Finder", windows: "File Explorer" }[info.data.platform.os] ?? "file manager"}
                </Button>
              </span>,
            ],
          ]}
        />
      ) : null}
    </Panel>
  );
}

function UpdatesRow() {
  const s = useUpdateState();
  const [auto, setAuto] = useState(autoCheckEnabled());
  if (!updatesSupported()) return <span className="text-muted">Available in the installed app.</span>;
  return (
    <span className="flex flex-col gap-1.5">
      <span className="flex flex-wrap items-center gap-2">
        <label className="flex items-center gap-1.5">
          <input
            type="checkbox"
            checked={auto}
            onChange={(e) => {
              setAutoCheck(e.target.checked);
              setAuto(e.target.checked);
            }}
          />
          Check for updates when AGASTHY starts
        </label>
        <Button size="sm" variant="ghost" disabled={s.status === "checking" || s.status === "installing"} onClick={() => void checkForUpdate()}>
          {s.status === "checking" ? "Checking…" : "Check now"}
        </Button>
        {s.status === "available" ? (
          <Button size="sm" variant="primary" onClick={() => void installUpdate()}>
            Install {s.version} and restart
          </Button>
        ) : null}
      </span>
      <span className="text-[11.5px] text-muted">
        {s.status === "none"
          ? "You have the latest version."
          : s.status === "error"
            ? `Could not check: ${s.message}`
            : "Asks github.com for the latest release. Nothing about your machine is sent; updates are signature-checked and only installed when you click Install."}
      </span>
    </span>
  );
}

function ReportRow() {
  const [open, setOpen] = useState(false);
  return (
    <span className="flex flex-wrap items-center gap-2">
      <Button size="sm" onClick={() => setOpen(true)}>
        Report a problem
      </Button>
      <span className="text-muted">Prepares a redacted summary you review before opening a GitHub issue.</span>
      {open ? <ReportDialog onClose={() => setOpen(false)} /> : null}
    </span>
  );
}
