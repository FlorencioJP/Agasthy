import type {
  BinaryAttrs,
  DependencyAttrs,
  Entity,
  EntityKind,
  Evidence,
  InstalledApp,
  PackageAttrs,
  PackageManagerAttrs,
  RelationKind,
  Relationship,
  RemoteRef,
  RepositoryAttrs,
} from "@agasthy/shared-types";
import { Badge, Button, ConfidenceBadge, StateView } from "@agasthy/ui";
import { Copy, FolderOpen, X } from "lucide-react";
import { useState, type ReactNode } from "react";
import { AiAttributes } from "../components/ai-attributes";
import { FindingAttributes } from "../components/finding-attributes";
import { Avail } from "../components/availability";
import { Kv } from "../components/page";
import { formatBytes, formatDateTime, formatRelative, shortenHome } from "../lib/format";
import { errorMessage, revealInFileManager } from "../lib/ipc";
import { useEntityDetail, useOsWords } from "../lib/queries";
import { KIND_LABEL } from "./discovery";

const REL_LABEL: Record<RelationKind, { out: string; in: string }> = {
  origin_remote: { out: "Origin remote", in: "Cloned locally as origin" },
  tracks_remote: { out: "Also tracks remote", in: "Tracked as a remote by" },
  managed_by: { out: "Managed by", in: "Manages" },
  upstream_project: { out: "Upstream project", in: "Upstream of package" },
  provides: { out: "Provides", in: "Installed by package" },
  duplicate_clone: { out: "Duplicate clone of", in: "Duplicate clone" },
  installed_by: { out: "Installed by package", in: "Provides executable" },
  depends_on: { out: "Depends on", in: "Used by project" },
  requires_runtime: { out: "Requires runtime", in: "Required by project" },
  contains: { out: "Contains", in: "In project" },
  configures_tool: { out: "Read by", in: "Configured by file" },
  declares_mcp_server: { out: "Declares MCP server", in: "Declared in" },
  uses_mcp_server: { out: "Uses MCP server", in: "Used by" },
  uses_provider: { out: "Uses provider", in: "Used by tool" },
  serves_model: { out: "Serves model", in: "Served by" },
  same_content: { out: "Identical to", in: "Identical to" },
  potential_conflict: { out: "May conflict with", in: "May conflict with" },
  references: { out: "Imports", in: "Imported by" },
  has_finding: { out: "Has finding", in: "Affects" },
};

export function EntityDetailPanel({
  id,
  onClose,
  onNavigate,
  actions,
  extraActions,
  extra,
}: {
  id: string;
  onClose: () => void;
  onNavigate: (id: string, kind: EntityKind) => void;
  /** Extra buttons shown under the header. */
  actions?: ReactNode;
  extraActions?: ReactNode;
  /** Extra sections rendered for the loaded entity (e.g. impact analysis). */
  extra?: (entity: Entity) => ReactNode;
}) {
  const detail = useEntityDetail(id);
  return (
    <aside className="flex w-[420px] shrink-0 flex-col overflow-y-auto border-l border-line bg-surface" aria-label="Details">
      <div className="flex h-9 shrink-0 items-center justify-between border-b border-line px-3.5">
        <span className="text-[12px] font-medium text-muted">Details</span>
        <Button variant="ghost" size="icon" aria-label="Close details" onClick={onClose}>
          <X />
        </Button>
      </div>
      {detail.isLoading ? (
        <StateView kind="loading" title="Loading" compact />
      ) : detail.error ? (
        <StateView kind="error" title="Could not load details" description={errorMessage(detail.error)} compact />
      ) : !detail.data ? (
        <StateView kind="empty" title="No longer present" description="This item was not found in the most recent scan." compact />
      ) : (
        <div className="flex flex-col gap-5 p-4">
          <Header entity={detail.data.entity} />
          {actions || extraActions ? (
            <div className="-mt-3 flex gap-1.5">
              {actions}
              {extraActions}
            </div>
          ) : null}
          {extra?.(detail.data.entity)}
          <Attributes entity={detail.data.entity} />
          <Relationships outgoing={detail.data.outgoing} incoming={detail.data.incoming} onNavigate={onNavigate} />
          <EvidenceList evidence={detail.data.evidence} />
          <div className="text-[11px] text-faint">
            First seen {formatDateTime(detail.data.entity.first_seen)} · last confirmed {formatRelative(detail.data.entity.last_seen)}
          </div>
        </div>
      )}
    </aside>
  );
}

function Header({ entity }: { entity: Entity }) {
  const words = useOsWords();
  const [copied, setCopied] = useState(false);
  const [revealError, setRevealError] = useState<string | null>(null);
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2">
        <Badge>{KIND_LABEL[entity.kind].short}</Badge>
        {entity.version ? <span className="font-mono text-[11.5px] text-muted">{entity.version}</span> : null}
      </div>
      <h2 className="selectable text-[15px] font-semibold text-fg">{entity.name}</h2>
      <div className="text-[12px] text-muted">{KIND_LABEL[entity.kind].provenance}</div>
      {entity.path ? (
        <div className="flex flex-col gap-1.5">
          <code className="selectable break-all rounded bg-raised px-2 py-1 font-mono text-[11.5px] text-fg">{entity.path}</code>
          <div className="flex gap-1.5">
            <Button
              size="sm"
              onClick={() => {
                setRevealError(null);
                revealInFileManager(entity.path as string).catch((e) => setRevealError(errorMessage(e)));
              }}
            >
              <FolderOpen aria-hidden /> Show in {words.fileManager}
            </Button>
            <Button
              size="sm"
              variant="ghost"
              onClick={() => {
                navigator.clipboard
                  .writeText(entity.path as string)
                  .then(() => {
                    setCopied(true);
                    setTimeout(() => setCopied(false), 1500);
                  })
                  .catch((e) => setRevealError(errorMessage(e)));
              }}
            >
              <Copy aria-hidden /> {copied ? "Copied" : "Copy path"}
            </Button>
          </div>
          {revealError ? <div className="text-[11.5px] text-danger">{revealError}</div> : null}
        </div>
      ) : null}
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <h3 className="text-[11px] font-medium uppercase tracking-[0.07em] text-faint">{title}</h3>
      {children}
    </section>
  );
}

function remoteText(r: RemoteRef | null): ReactNode {
  if (!r) return <span className="text-faint">—</span>;
  return <span className="selectable">{r.web_url ?? `${r.host}/${r.path}`}</span>;
}

function Attributes({ entity }: { entity: Entity }) {
  const words = useOsWords();
  const a = entity.attributes as Record<string, unknown>;
  switch (entity.kind) {
    case "ai_tool":
    case "ai_provider":
    case "local_model":
    case "mcp_server":
    case "ai_config_file":
      return <AiAttributes entity={entity} />;
    case "finding":
      return <FindingAttributes entity={entity} />;
    case "repository": {
      const r = a as unknown as RepositoryAttrs;
      return (
        <Section title="Repository">
          <Kv
            items={[
              ["Branch", <Avail value={r.head} render={(h) => (h.unborn ? "no commits yet" : h.detached ? "detached HEAD" : h.branch)} />],
              ["Commit", <Avail value={r.head} render={(h) => <code className="font-mono text-[11.5px]">{h.commit_sha ?? "—"}</code>} />],
              ["Last commit", <Avail value={r.head} render={(h) => (h.last_commit_at ? formatDateTime(h.last_commit_at) : "—")} />],
              [
                "Working tree",
                <Avail
                  value={r.working_tree}
                  render={(w) => (w.clean ? "clean" : `${w.staged} staged · ${w.modified} modified · ${w.untracked} untracked${w.conflicted ? ` · ${w.conflicted} conflicted` : ""}`)}
                />,
              ],
              ["Local branches", <Avail value={r.local_branches} render={(n) => n} />],
              ["Primary remote", remoteText(r.primary_remote)],
              ["Clones of remote", r.primary_remote ? `${r.clones_of_same_remote} on this machine` : "—"],
              ["Languages", r.languages.length ? r.languages.slice(0, 5).map((l) => `${l.language} (${l.files})`).join(", ") : "—"],
              ["Frameworks", r.frameworks.length ? r.frameworks.join(", ") : "none detected"],
              ["License", r.license ? `${r.license.spdx ?? "unrecognised"} · ${r.license.source}` : "none found"],
              ["Manifests", r.manifests.length ? r.manifests.join(", ") : "none"],
              ["Size on disk", <Avail value={r.size_bytes} render={(b) => formatBytes(b)} />],
              ["Dependencies", r.dependency_count != null ? `${r.dependency_count} declared` : "—"],
              ["Runtimes", r.runtime_requirements?.length ? r.runtime_requirements.join(", ") : "none declared"],
              ["Git dir", r.git_dir_kind === "file" ? "linked worktree or submodule" : "standard clone"],
              ["Scan folder", shortenHome(r.scan_root)],
            ]}
          />
          {r.remotes.length ? (
            <div className="mt-1 flex flex-col gap-1">
              {r.remotes.map((rm) => (
                <div key={rm.name} className="flex items-center gap-2 text-[12px]">
                  <span className="w-20 shrink-0 font-mono text-muted">{rm.name}</span>
                  <span className="selectable truncate text-fg" title={rm.url}>
                    {rm.url}
                  </span>
                  {rm.credentials_redacted ? <Badge tone="warning" title="Credentials embedded in this URL were removed before storing">redacted</Badge> : null}
                </div>
              ))}
            </div>
          ) : null}
        </Section>
      );
    }
    case "application": {
      const x = a as unknown as InstalledApp;
      return (
        <Section title="Application">
          <Kv
            items={[
              ["Bundle ID", x.bundle_id ?? "—"],
              ["Version", x.version ?? "—"],
              ["Build", x.build ?? "—"],
              ["Location", x.location === "system" ? `shipped with ${words.osName}` : x.location === "user" ? (words.os === "macos" ? "user Applications folder" : "installed for this user") : words.os === "macos" ? "/Applications (all users)" : "installed for all users"],
            ]}
          />
        </Section>
      );
    }
    case "binary":
    case "runtime": {
      const b = a as unknown as BinaryAttrs;
      return (
        <Section title={entity.kind === "runtime" ? "Runtime" : "Tool"}>
          <Kv
            items={[
              ["Version", <Avail value={b.version} render={(v) => v} />],
              ["Version command", b.version_command ? <code className="font-mono text-[11.5px]">{b.version_command}</code> : "not run"],
              ["Category", b.category.replace("_", " ")],
              ["Resolution", b.active ? "first match in AGASTHY's search order" : `shadowed by ${b.shadowed_by ?? "another copy"}`],
              ["Directory", `${shortenHome(b.directory)} (${b.directory_origin === "process_path" ? "on PATH" : b.directory_origin === "system_path_config" ? "/etc/paths" : "conventional location"})`],
              ["Resolves to", b.resolved_path ? <span className="selectable break-all">{b.resolved_path}</span> : "not a symlink"],
              ["Installed via", b.install_hint ? `${b.install_hint.installer} — ${b.install_hint.detail}` : "unknown"],
            ]}
          />
          {!b.active ? null : (
            <p className="text-[11.5px] leading-5 text-faint">
              A terminal with a different PATH may resolve a different copy. AGASTHY searches its own PATH, then /etc/paths, then conventional locations.
            </p>
          )}
        </Section>
      );
    }
    case "package": {
      const p = a as unknown as PackageAttrs;
      return (
        <Section title="Package">
          <Kv
            items={[
              ["Manager", p.manager],
              ["Type", p.package_kind],
              ["Description", p.description ?? "—"],
              ["Homepage", p.homepage ? <span className="selectable break-all">{p.homepage}</span> : "—"],
              ["License", p.license ?? "—"],
              ["Requested", p.installed_on_request == null ? "—" : p.installed_on_request ? "installed on request" : "installed as a dependency"],
              ["Provides", p.provides.length ? p.provides.join(", ") : "—"],
              ["Upstream", remoteText(p.upstream)],
            ]}
          />
        </Section>
      );
    }
    case "package_manager": {
      const m = a as unknown as PackageManagerAttrs;
      const s = m.state;
      return (
        <Section title="Package manager">
          <Kv
            items={[
              ["State", s.state === "read" ? "inventory read" : s.state === "not_installed" ? "not installed" : s.state === "not_implemented" ? "installed; inventory not supported in this build" : `failed: ${s.message}`],
              ["Packages", m.package_count ?? "—"],
              ["Inventory command", m.inventory_command ? <code className="font-mono text-[11.5px]">{m.inventory_command}</code> : "—"],
            ]}
          />
        </Section>
      );
    }
    case "dependency": {
      const d = a as unknown as DependencyAttrs;
      return (
        <Section title="Declared dependency">
          <Kv items={[["Ecosystem", d.ecosystem], ["Package", d.package]]} />
          <p className="text-[11.5px] leading-5 text-faint">
            Declared in project manifests. This does not mean it is installed; versions shown below come from lockfiles when present.
          </p>
        </Section>
      );
    }
    case "remote_project": {
      const r = a as unknown as RemoteRef;
      return (
        <Section title="Upstream project">
          <Kv items={[["Host", r.host], ["Path", r.path], ["URL", remoteText(r)]]} />
        </Section>
      );
    }
  }
}

function Relationships({
  outgoing,
  incoming,
  onNavigate,
}: {
  outgoing: Relationship[];
  incoming: Relationship[];
  onNavigate: (id: string, kind: EntityKind) => void;
}) {
  const [showAll, setShowAll] = useState(false);
  // Most informative first: provenance and runtime links before long dependency lists.
  const rank = (k: RelationKind) => (k === "depends_on" ? 2 : k === "managed_by" ? 1 : 0);
  const all = [...outgoing.map((r) => ({ r, dir: "out" as const })), ...incoming.map((r) => ({ r, dir: "in" as const }))].sort(
    (a, b) => rank(a.r.kind) - rank(b.r.kind),
  );
  const LIMIT = 15;
  const rows = showAll ? all : all.slice(0, LIMIT);
  return (
    <Section title={`Relationships (${all.length})`}>
      {rows.length === 0 ? (
        <div className="text-[12px] text-faint">No relationships established from the available evidence.</div>
      ) : (
        <ul className="flex flex-col gap-1.5">
          {rows.map(({ r, dir }) => (
            <li key={r.id + dir} className="rounded-md border border-line px-2.5 py-2">
              <div className="flex items-center justify-between gap-2">
                <span className="text-[11.5px] text-muted">{REL_LABEL[r.kind][dir]}</span>
                <ConfidenceBadge level={r.confidence} />
              </div>
              {r.other ? (
                r.other.kind === "remote_project" ? (
                  <div className="mt-0.5 truncate text-[12.5px] text-fg">{r.other.name}</div>
                ) : (
                  <button className="mt-0.5 truncate text-left text-[12.5px] text-fg underline-offset-2 hover:underline" onClick={() => onNavigate(r.other!.id, r.other!.kind)}>
                    {r.other.name}
                    {r.other.path ? <span className="ml-1.5 text-faint">{shortenHome(r.other.path)}</span> : null}
                  </button>
                )
              ) : null}
              {r.evidence.map((ev) => (
                <div key={ev.id} className="mt-1 text-[11.5px] text-faint">
                  {ev.detail}
                </div>
              ))}
            </li>
          ))}
          {all.length > LIMIT ? (
            <li>
              <button className="text-[12px] text-accent underline-offset-2 hover:underline" onClick={() => setShowAll((v) => !v)}>
                {showAll ? "Show fewer" : `Show all ${all.length}`}
              </button>
            </li>
          ) : null}
        </ul>
      )}
    </Section>
  );
}

function EvidenceList({ evidence }: { evidence: Evidence[] }) {
  return (
    <Section title={`Evidence (${evidence.length})`}>
      {evidence.length === 0 ? (
        <div className="text-[12px] text-faint">No evidence recorded.</div>
      ) : (
        <ul className="flex flex-col gap-1.5">
          {evidence.map((ev) => (
            <li key={ev.id} className="flex flex-col gap-0.5 rounded-md bg-raised px-2.5 py-2">
              <div className="flex items-center justify-between gap-2">
                <span className="text-[12px] text-fg">{ev.detail}</span>
                <ConfidenceBadge level={ev.confidence} />
              </div>
              <code className="selectable break-all font-mono text-[11px] text-faint">{ev.source}</code>
            </li>
          ))}
        </ul>
      )}
    </Section>
  );
}
