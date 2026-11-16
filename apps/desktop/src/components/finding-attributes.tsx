import type { Entity, FindingAttrs, Severity } from "@agasthy/shared-types";
import { Badge, type BadgeProps } from "@agasthy/ui";
import { Kv } from "./page";

export const SEVERITY: Record<Severity, { label: string; tone: BadgeProps["tone"] }> = {
  critical: { label: "critical", tone: "danger" },
  high: { label: "high", tone: "danger" },
  medium: { label: "medium", tone: "warning" },
  low: { label: "low", tone: "info" },
  info: { label: "info", tone: "neutral" },
  unknown: { label: "severity unknown", tone: "neutral" },
};

export function FindingAttributes({ entity }: { entity: Entity }) {
  const f = entity.attributes as unknown as FindingAttrs;
  return (
    <section className="flex flex-col gap-2">
      <h3 className="text-[11px] font-medium uppercase tracking-[0.07em] text-faint">Finding</h3>
      <div className="flex flex-wrap gap-1.5">
        <Badge tone={SEVERITY[f.severity].tone}>{SEVERITY[f.severity].label}</Badge>
        <Badge>{f.category}</Badge>
        <Badge>source: {f.source}</Badge>
      </div>
      <p className="selectable text-[12.5px] leading-5 text-fg">{f.summary}</p>
      <Kv
        items={[
          ["ID", <span className="selectable font-mono text-[11.5px]">{f.finding_id}</span>],
          ["Also known as", f.aliases.length ? f.aliases.join(", ") : "—"],
          ["Affects", f.packages.join(", ") || "—"],
          ["Fixed in", f.fixed_versions.length ? f.fixed_versions.join(", ") : "not listed"],
          ["Remediation", f.remediation],
          ["Published", f.published ?? "—"],
        ]}
      />
      {f.references.length ? (
        <ul className="flex flex-col gap-0.5">
          {f.references.map((r) => (
            <li key={r} className="selectable break-all font-mono text-[11px] text-muted">
              {r}
            </li>
          ))}
        </ul>
      ) : null}
    </section>
  );
}
