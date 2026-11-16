import type { ActionRequest, Entity, ImpactReport, Lab, LabEcosystem, RepositoryAttrs, RiskLevel } from "@agasthy/shared-types";
import { Badge, Button, StateView, cn, type BadgeProps } from "@agasthy/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate, useSearch } from "@tanstack/react-router";
import { FlaskConical, Search } from "lucide-react";
import { useState } from "react";
import { ActionDialog } from "../components/action-dialog";
import { PageHeader, Panel } from "../components/page";
import { formatDateTime, formatRelative, shortenHome } from "../lib/format";
import { api, errorMessage } from "../lib/ipc";
import { useEntities } from "../lib/queries";
import { KIND_LABEL } from "./discovery";

export interface ImpactSearch {
  tab: "impact" | "lab";
  id?: string;
}

const RISK: Record<RiskLevel, { label: string; tone: BadgeProps["tone"] }> = {
  low: { label: "low risk", tone: "success" },
  medium: { label: "review first", tone: "warning" },
  high: { label: "high risk", tone: "danger" },
  unknown: { label: "unknown effects", tone: "warning" },
};

export function ImpactPage() {
  const search = useSearch({ from: "/impact" });
  const nav = useNavigate({ from: "/impact" });
  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader title="Impact Analysis" subtitle="Simulations only. Nothing is removed or updated from this page without a separate review." />
      <div className="flex shrink-0 gap-1 border-b border-line px-5 py-2" role="tablist">
        {(
          [
            ["impact", "What if I remove…"],
            ["lab", "Test an update (Update Lab)"],
          ] as const
        ).map(([id, label]) => (
          <button
            key={id}
            role="tab"
            aria-selected={search.tab === id}
            onClick={() => void nav({ search: (s) => ({ ...s, tab: id }) })}
            className={cn("h-7 rounded-md px-2.5 text-[12.5px]", search.tab === id ? "bg-hover text-fg" : "text-muted hover:text-fg")}
          >
            {label}
          </button>
        ))}
      </div>
      <div className="min-h-0 flex-1 overflow-y-auto p-5">{search.tab === "impact" ? <RemovalView /> : <LabView />}</div>
    </div>
  );
}

function EntityPicker({ onPick }: { onPick: (e: Entity) => void }) {
  const [text, setText] = useState("");
  const q = useEntities(null, text, 12);
  const rows = text.trim() ? (q.data ?? []).filter((e) => ["package", "runtime", "binary", "dependency", "application", "ai_tool"].includes(e.kind)) : [];
  return (
    <div className="relative w-full max-w-xl">
      <label className="flex h-8 items-center gap-1.5 rounded-md border border-line bg-surface px-2 focus-within:border-accent/60">
        <Search className="size-3.5 text-faint" aria-hidden />
        <input
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="Package, runtime, tool, dependency or app (e.g. python, node, jq)"
          aria-label="Find something to analyse"
          className="min-w-0 flex-1 bg-transparent text-[12.5px] text-fg outline-none placeholder:text-faint"
        />
      </label>
      {rows.length ? (
        <ul className="absolute z-20 mt-1 w-full overflow-hidden rounded-md border border-line bg-surface shadow-lg">
          {rows.map((e) => (
            <li key={e.id}>
              <button
                className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left text-[12.5px] hover:bg-hover"
                onClick={() => {
                  onPick(e);
                  setText("");
                }}
              >
                <Badge>{KIND_LABEL[e.kind].short}</Badge>
                <span className="truncate text-fg">{e.name}</span>
                <span className="ml-auto truncate font-mono text-[11px] text-faint">{e.version ?? (e.path ? shortenHome(e.path) : "")}</span>
              </button>
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}

function RuntimeReportView({ r }: { r: ImpactReport }) {
  const tone: Record<string, BadgeProps["tone"]> = { breaks: "danger", unknown: "warning", falls_back: "info", already_incompatible: "neutral", unaffected: "success" };
  return (
    <div className="flex flex-col gap-1.5">
      <div className="text-[12px] text-muted">
        {r.runtime} {r.target.version} at <span className="font-mono">{shortenHome(r.target.path ?? "")}</span>: {r.breaks} would break · {r.unknown} unknown · {r.falls_back} fall back
      </div>
      {r.items.map((i) => (
        <div key={i.project.id} className="flex items-start justify-between gap-2 rounded-md border border-line px-2.5 py-1.5">
          <div className="min-w-0">
            <div className="text-[12.5px] text-fg">{i.project.name}</div>
            <div className="text-[11.5px] text-muted">{i.explanation}</div>
            <div className="font-mono text-[11px] text-faint">{i.requirements.join(" · ")}</div>
          </div>
          <Badge tone={tone[i.outcome]}>{i.outcome.replaceAll("_", " ")}</Badge>
        </div>
      ))}
    </div>
  );
}

function RemovalView() {
  const search = useSearch({ from: "/impact" });
  const nav = useNavigate({ from: "/impact" });
  const q = useQuery({ queryKey: ["removal-impact", search.id], queryFn: () => api.removalImpact(search.id as string), enabled: Boolean(search.id) });
  const r = q.data;
  return (
    <div className="flex max-w-4xl flex-col gap-4">
      <EntityPicker onPick={(e) => void nav({ search: (s) => ({ ...s, id: e.id }) })} />
      {!search.id ? (
        <StateView
          kind="empty"
          title="Choose something to analyse"
          description="AGASTHY follows recorded, evidence-backed relationships (Homebrew dependencies, executables that resolve into a package, runtime requirements in your projects, manifest declarations) to show what would be affected. Anything it cannot see is listed as unknown."
        />
      ) : q.isLoading ? (
        <StateView kind="loading" title="Following relationships" />
      ) : q.error ? (
        <StateView kind="error" title="Analysis failed" description={errorMessage(q.error)} />
      ) : !r ? (
        <StateView kind="empty" title="Not found" description="It is no longer in AGASTHY's data; rescan." />
      ) : (
        <>
          <Panel title={`Removing ${r.target.name}`} actions={<Badge tone={RISK[r.risk].tone}>{RISK[r.risk].label}</Badge>}>
            <p className="text-[13px] text-fg">{r.summary}</p>
            <p className="mt-1 text-[11.5px] text-faint">{KIND_LABEL[r.target.kind].provenance}</p>
          </Panel>
          {r.sections.map((s) => (
            <Panel key={s.title} title={`${s.title} (${s.entries.length})`}>
              <ul className="divide-y divide-line">
                {s.entries.map((e) => (
                  <li key={e.entity.id} className="flex items-center justify-between gap-2 py-1.5">
                    <button className="min-w-0 text-left hover:underline" onClick={() => void nav({ search: (x) => ({ ...x, id: e.entity.id }) })}>
                      <span className="text-[12.5px] text-fg">{e.entity.name}</span> <span className="text-[11.5px] text-muted">{e.explanation}</span>
                    </button>
                    <Badge>{KIND_LABEL[e.entity.kind].short}</Badge>
                  </li>
                ))}
              </ul>
            </Panel>
          ))}
          {r.runtime_reports.length ? (
            <Panel title="Projects that use the runtime">
              <div className="flex flex-col gap-3">
                {r.runtime_reports.map((rep) => (
                  <RuntimeReportView key={rep.target.id} r={rep} />
                ))}
              </div>
            </Panel>
          ) : null}
          <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
            <Panel title="Suggested plan (AGASTHY does not run these)">
              {r.plan.length ? (
                <ol className="list-decimal pl-4 text-[12.5px] leading-6 text-fg">
                  {r.plan.map((p) => (
                    <li key={p} className="selectable">
                      {p}
                    </li>
                  ))}
                </ol>
              ) : (
                <div className="text-[12px] text-faint">No specific steps.</div>
              )}
            </Panel>
            <Panel title="Recovery">
              {r.recovery.length ? (
                <ul className="list-disc pl-4 text-[12.5px] leading-6 text-fg">
                  {r.recovery.map((p) => (
                    <li key={p} className="selectable">
                      {p}
                    </li>
                  ))}
                </ul>
              ) : (
                <div className="text-[12px] text-faint">—</div>
              )}
            </Panel>
          </div>
          <Panel title="Unknowns">
            <ul className="list-disc pl-4 text-[12px] leading-6 text-muted">
              {r.unknowns.map((u) => (
                <li key={u}>{u}</li>
              ))}
            </ul>
          </Panel>
        </>
      )}
    </div>
  );
}

const ECOS: { id: LabEcosystem; label: string; manifest: string }[] = [
  { id: "npm", label: "npm", manifest: "package.json" },
  { id: "pnpm", label: "pnpm", manifest: "package.json" },
  { id: "cargo", label: "Cargo", manifest: "Cargo.toml" },
];

function LabView() {
  const qc = useQueryClient();
  const repos = useEntities("repository", "");
  const labs = useQuery({ queryKey: ["labs"], queryFn: api.labsList });
  const [repoId, setRepoId] = useState("");
  const [eco, setEco] = useState<LabEcosystem>("npm");
  const [pkg, setPkg] = useState("");
  const [version, setVersion] = useState("");
  const [action, setAction] = useState<ActionRequest | null>(null);
  const repo = (repos.data ?? []).find((r) => r.id === repoId);
  const manifests = repo ? (repo.attributes as unknown as RepositoryAttrs).manifests : [];
  const allowed = ECOS.filter((e) => manifests.includes(e.manifest));
  // Use a package manager the selected repository actually has a manifest for.
  const ecosystem: LabEcosystem = allowed.length && !allowed.some((a) => a.id === eco) ? allowed[0].id : eco;

  const close = () => {
    setAction(null);
    void qc.invalidateQueries({ queryKey: ["labs"] });
  };

  return (
    <div className="flex max-w-4xl flex-col gap-4">
      <Panel title="New lab">
        <p className="mb-3 text-[12px] leading-5 text-muted">
          A lab is a separate copy of your project at its last commit (a Git worktree in AGASTHY's data folder). The new version is written only there. You then
          choose, one approved step at a time, whether to install and run tests in it, and whether to copy the result back.
        </p>
        {repos.data?.length === 0 ? (
          <StateView kind="empty" title="No repositories" description="Add project folders in Settings and scan them first." compact />
        ) : (
          <div className="grid grid-cols-1 gap-2 md:grid-cols-[1.4fr_0.7fr_1fr_0.8fr_auto]">
            <select aria-label="Repository" value={repoId} onChange={(e) => setRepoId(e.target.value)} className="h-8 rounded-md border border-line bg-surface px-2 text-[12.5px] text-fg">
              <option value="">Repository…</option>
              {(repos.data ?? []).map((r) => (
                <option key={r.id} value={r.id}>
                  {r.name}
                </option>
              ))}
            </select>
            <select aria-label="Package manager" value={ecosystem} onChange={(e) => setEco(e.target.value as LabEcosystem)} className="h-8 rounded-md border border-line bg-surface px-2 text-[12.5px] text-fg">
              {(allowed.length ? allowed : ECOS).map((e) => (
                <option key={e.id} value={e.id}>
                  {e.label}
                </option>
              ))}
            </select>
            <input aria-label="Package" value={pkg} onChange={(e) => setPkg(e.target.value)} placeholder="package (e.g. react)" className="h-8 rounded-md border border-line bg-surface px-2 text-[12.5px] text-fg placeholder:text-faint" />
            <input aria-label="Target version" value={version} onChange={(e) => setVersion(e.target.value)} placeholder="version (e.g. 19.1.0)" className="h-8 rounded-md border border-line bg-surface px-2 text-[12.5px] text-fg placeholder:text-faint" />
            <Button
              variant="primary"
              disabled={!repoId || !pkg.trim() || !version.trim()}
              onClick={() => setAction({ type: "create_update_lab", repo_id: repoId, ecosystem, package: pkg.trim(), target_version: version.trim() })}
            >
              <FlaskConical aria-hidden /> Review…
            </Button>
          </div>
        )}
      </Panel>
      <Panel title="Labs" bodyClassName="p-0">
        {labs.isLoading ? (
          <StateView kind="loading" title="Loading" compact />
        ) : labs.error ? (
          <StateView kind="error" title="Could not read labs" description={errorMessage(labs.error)} compact />
        ) : !labs.data?.length ? (
          <StateView kind="empty" title="No labs yet" compact />
        ) : (
          <ul className="divide-y divide-line">
            {labs.data.map((l) => (
              <LabRow key={l.id} lab={l} onAction={setAction} />
            ))}
          </ul>
        )}
      </Panel>
      {action ? <ActionDialog request={action} onClose={close} /> : null}
    </div>
  );
}

function LabRow({ lab, onAction }: { lab: Lab; onAction: (a: ActionRequest) => void }) {
  const [open, setOpen] = useState(false);
  const active = lab.state === "ready";
  const installed = lab.steps.some((s) => s.step === "install" && s.ok);
  return (
    <li className="px-3.5 py-2.5">
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <div className="text-[12.5px] text-fg">
            {lab.spec.package} {lab.old_constraint} → <span className="font-medium">{lab.new_constraint}</span> in {lab.spec.repo_path.split("/").pop()}{" "}
            <Badge tone={lab.state === "ready" ? "accent" : lab.state === "applied" ? "success" : "neutral"}>{lab.state}</Badge>
          </div>
          <div className="text-[11.5px] text-faint">
            {lab.spec.ecosystem} · created {formatRelative(lab.created_at)} · base {lab.base_commit.slice(0, 10)}
          </div>
        </div>
        <div className="flex shrink-0 flex-wrap justify-end gap-1.5">
          {active ? (
            <>
              <Button size="sm" onClick={() => onAction({ type: "run_lab_step", lab_id: lab.id, step: "install", allow_scripts: false })}>
                Install…
              </Button>
              <Button size="sm" variant="ghost" onClick={() => onAction({ type: "run_lab_step", lab_id: lab.id, step: "install", allow_scripts: true })}>
                Install with scripts…
              </Button>
              <Button size="sm" disabled={!installed} onClick={() => onAction({ type: "run_lab_step", lab_id: lab.id, step: "test", allow_scripts: false })}>
                Run tests…
              </Button>
              <Button size="sm" variant="primary" disabled={!installed} onClick={() => onAction({ type: "apply_lab", lab_id: lab.id })}>
                Apply…
              </Button>
            </>
          ) : null}
          {lab.state !== "discarded" ? (
            <Button size="sm" variant="ghost" onClick={() => onAction({ type: "discard_lab", lab_id: lab.id })}>
              Discard…
            </Button>
          ) : null}
          <Button size="sm" variant="ghost" onClick={() => setOpen((o) => !o)}>
            {open ? "Hide" : "Details"}
          </Button>
        </div>
      </div>
      {open ? (
        <div className="mt-2 flex flex-col gap-2">
          {lab.steps.length === 0 ? <div className="text-[12px] text-faint">No steps run yet.</div> : null}
          {lab.steps.map((s, i) => (
            <div key={i} className="rounded-md border border-line p-2">
              <div className="flex items-center justify-between text-[12px]">
                <span className="font-mono text-fg">{s.command}</span>
                <Badge tone={s.ok ? "success" : "danger"}>{s.ok ? "passed" : `failed (${s.exit_code ?? "?"})`}</Badge>
              </div>
              <div className="text-[11px] text-faint">{formatDateTime(s.at)}</div>
              <pre className="selectable mt-1 max-h-48 overflow-auto whitespace-pre-wrap rounded bg-raised p-2 font-mono text-[11px] text-muted">{s.output_tail}</pre>
            </div>
          ))}
          {lab.changes ? <pre className="selectable max-h-64 overflow-auto whitespace-pre-wrap rounded bg-raised p-2 font-mono text-[11px] text-muted">{lab.changes}</pre> : null}
          <div className="font-mono text-[11px] text-faint">{shortenHome(lab.worktree)}</div>
        </div>
      ) : null}
    </li>
  );
}
