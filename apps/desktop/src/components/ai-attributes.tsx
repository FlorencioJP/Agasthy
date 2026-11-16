import type {
  AiConfigFileAttrs,
  AiProviderAttrs,
  AiToolAttrs,
  Entity,
  LocalModelAttrs,
  McpServerAttrs,
  ProviderLevel,
} from "@agasthy/shared-types";
import { Badge, type BadgeProps } from "@agasthy/ui";
import type { ReactNode } from "react";
import { formatBytes, formatDateTime, shortenHome } from "../lib/format";
import { Kv } from "./page";

export const PROVIDER_LEVEL: Record<ProviderLevel, { label: string; tone: BadgeProps["tone"]; hint: string }> = {
  referenced: { label: "Referenced", tone: "neutral", hint: "A tool on this machine is built for this provider" },
  configured: { label: "Configured", tone: "info", hint: "A configuration file names this provider" },
  endpoint_configured: { label: "Endpoint configured", tone: "info", hint: "An endpoint for this provider is configured" },
  credential_present: { label: "Credential present", tone: "accent", hint: "A credential appears to be present (only its presence was checked)" },
  reachable: { label: "Reachable", tone: "success", hint: "A local endpoint accepted a connection" },
};

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="flex flex-col gap-2">
      <h3 className="text-[11px] font-medium uppercase tracking-[0.07em] text-faint">{title}</h3>
      {children}
    </section>
  );
}

const list = (xs: string[], empty = "none") => (xs.length ? xs.join(", ") : empty);

/** Detail view for AI ecosystem entities. Returns null for other kinds. */
export function AiAttributes({ entity }: { entity: Entity }): ReactNode {
  const a = entity.attributes as unknown;
  switch (entity.kind) {
    case "ai_tool": {
      const t = a as AiToolAttrs;
      return (
        <Section title="AI tool">
          <Kv
            items={[
              ["Vendor", t.vendor],
              ["Category", t.category.replaceAll("_", " ")],
              ["Installed", t.installed ? "yes (executable, application or extension found)" : "no: configuration only, possibly left over"],
              [
                "Local endpoint",
                t.local_endpoint ? `${t.local_endpoint.address} ${t.local_endpoint.listening ? "is accepting connections" : "is not listening"}` : "—",
              ],
            ]}
          />
          <ul className="flex flex-col gap-1">
            {t.signals.map((s) => (
              <li key={s.kind + s.path} className="text-[12px]">
                <Badge>{s.kind.replace("_", " ")}</Badge> <span className="selectable font-mono text-[11.5px] text-muted">{shortenHome(s.path)}</span>
                {s.detail ? <span className="text-faint"> · {s.detail}</span> : null}
              </li>
            ))}
          </ul>
        </Section>
      );
    }
    case "ai_provider": {
      const p = a as AiProviderAttrs;
      return (
        <Section title="Provider">
          <div className="flex items-center gap-2">
            <Badge tone={PROVIDER_LEVEL[p.highest].tone}>{PROVIDER_LEVEL[p.highest].label}</Badge>
            <span className="text-[12px] text-muted">{p.kind === "local" ? "local inference" : "cloud API"}</span>
          </div>
          <ul className="flex flex-col gap-1.5">
            {p.signals.map((s, i) => (
              <li key={i} className="text-[12px]">
                <Badge tone={PROVIDER_LEVEL[s.level].tone} title={PROVIDER_LEVEL[s.level].hint}>
                  {PROVIDER_LEVEL[s.level].label}
                </Badge>{" "}
                <span className="text-fg">{s.detail}</span>
                <div className="selectable font-mono text-[11px] text-faint">{shortenHome(s.source)}</div>
              </li>
            ))}
          </ul>
          <p className="text-[11.5px] leading-5 text-faint">
            Connectivity to cloud providers is never tested and credential values are never read. AGASTHY was started from the desktop, so environment
            variables set only in your shell profile are not visible to it.
          </p>
        </Section>
      );
    }
    case "local_model": {
      const m = a as LocalModelAttrs;
      return (
        <Section title="Local model">
          <Kv
            items={[
              ["Store", m.store],
              ["Model", m.model],
              ["Tag / file", m.tag ?? "—"],
              ["Format", m.format ?? "—"],
              ["Size", m.size_bytes != null ? formatBytes(m.size_bytes) : "not measured"],
            ]}
          />
        </Section>
      );
    }
    case "mcp_server": {
      const s = a as McpServerAttrs;
      return (
        <Section title="MCP server">
          <Kv
            items={[
              ["Transport", s.transport],
              ["Command", s.command ? <code className="selectable break-all font-mono text-[11.5px]">{s.command}</code> : "—"],
              ["Arguments", s.args.length ? <code className="selectable break-all font-mono text-[11.5px]">{s.args.join(" ")}</code> : "none"],
              ["URL", s.url ?? "—"],
              ["Environment", list(s.env_names, "none") + (s.env_names.length ? " (names only; values are never stored)" : "")],
              ["Scope", s.scope === "global" ? "all projects" : `project ${shortenHome(s.scope)}`],
              ["Configured for", s.tool ?? "—"],
              ["State", s.disabled ? "disabled" : "enabled"],
              ["Config file", <span className="selectable break-all">{shortenHome(s.config_file)}</span>],
            ]}
          />
        </Section>
      );
    }
    case "ai_config_file": {
      const f = a as AiConfigFileAttrs;
      return (
        <>
          <Section title="AI file">
            <div className="flex flex-wrap gap-1.5">
              <Badge>{f.file_type.replace("_", " ")}</Badge>
              <Badge>{f.scope}</Badge>
              {f.orphaned ? <Badge tone="warning" title="No tool that reads this file was found on this machine">orphaned</Badge> : null}
              {f.duplicates.length ? <Badge tone="info">{f.duplicates.length} identical</Badge> : null}
              {f.overlaps.length ? <Badge tone="info">overlaps {f.overlaps.length}</Badge> : null}
              {f.redactions ? <Badge tone="accent">{f.redactions} redacted</Badge> : null}
            </div>
            <Kv
              items={[
                ["Read by", list(f.tools)],
                ["Project", f.project ? shortenHome(f.project) : "— (global)"],
                ["Path in project", f.relative_path],
                ["Size", `${formatBytes(f.size_bytes)} · ${f.lines} lines`],
                ["Modified", f.modified_at ? formatDateTime(f.modified_at) : "—"],
                ["Overlaps with", list(f.overlaps)],
                ["Identical to", f.duplicates.length ? f.duplicates.map((d) => shortenHome(d)).join(", ") : "none"],
                ["Imports", f.references.length ? f.references.map((r) => shortenHome(r)).join(", ") : "none"],
                [
                  "Stated settings",
                  f.directives.length ? [...new Set(f.directives.map((d) => `${d.topic.replace("_", " ")}: ${d.value}`))].join(", ") : "none detected",
                ],
              ]}
            />
          </Section>
          {f.text != null ? (
            <Section title="Contents">
              <p className="text-[11.5px] leading-5 text-faint">
                Shown as data. AGASTHY never follows instructions in these files. Credential-like values are redacted.
                {f.text_truncated ? " Truncated to the first 16 KB." : ""}
              </p>
              <pre className="selectable max-h-80 overflow-auto whitespace-pre-wrap rounded-md bg-raised p-2.5 font-mono text-[11.5px] leading-[1.45] text-fg">{f.text}</pre>
            </Section>
          ) : (
            <p className="text-[11.5px] leading-5 text-faint">
              Configuration files may contain credentials, so their contents are not stored. Only the information shown above was extracted.
            </p>
          )}
        </>
      );
    }
    default:
      return null;
  }
}
