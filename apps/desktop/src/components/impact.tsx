import type { ImpactOutcome } from "@agasthy/shared-types";
import { Badge, StateView, type BadgeProps } from "@agasthy/ui";
import { errorMessage } from "../lib/ipc";
import { shortenHome } from "../lib/format";
import { useRuntimeImpact } from "../lib/queries";

const OUTCOME: Record<ImpactOutcome, { label: string; tone: BadgeProps["tone"] }> = {
  breaks: { label: "Would break", tone: "danger" },
  unknown: { label: "Unknown", tone: "warning" },
  falls_back: { label: "Falls back", tone: "info" },
  already_incompatible: { label: "Already incompatible", tone: "neutral" },
  unaffected: { label: "Unaffected", tone: "success" },
};

/**
 * "What happens if I remove this runtime?" A simulation only: nothing is
 * removed. Results come from declared requirements and PATH order.
 */
export function RuntimeImpact({ id, onSelectProject }: { id: string; onSelectProject?: (id: string) => void }) {
  const q = useRuntimeImpact(id);
  if (q.isLoading) return <StateView kind="loading" title="Analysing dependents" compact />;
  if (q.error) return <StateView kind="error" title="Impact analysis failed" description={errorMessage(q.error)} compact />;
  const r = q.data;
  if (!r) return <StateView kind="unsupported" title="Impact analysis is available for Node.js, Python, Rust, Go and Ruby runtimes" compact />;
  return (
    <div className="flex flex-col gap-3" aria-label="Impact analysis">
      <div className="text-[12px] leading-5 text-muted">
        If <code className="font-mono text-fg">{shortenHome(r.target.path ?? r.target.id)}</code>
        {r.target.version ? ` (${r.runtime} ${r.target.version})` : ""} were removed from PATH:
      </div>
      <div className="flex flex-wrap gap-1.5">
        <Badge tone={r.breaks ? "danger" : "neutral"}>{r.breaks} would break</Badge>
        <Badge tone={r.unknown ? "warning" : "neutral"}>{r.unknown} unknown</Badge>
        <Badge tone="info">{r.falls_back} fall back</Badge>
        <Badge tone="success">{r.unaffected} unaffected</Badge>
      </div>
      {r.other_copies.length ? (
        <div className="text-[11.5px] text-faint">
          Other {r.runtime} copies, in resolution order:{" "}
          {r.other_copies.map((c) => `${shortenHome(c.path ?? c.id)} ${c.version ?? "(version unknown)"}`).join(" → ")}
        </div>
      ) : (
        <div className="text-[11.5px] text-faint">No other {r.runtime} copy was found on PATH.</div>
      )}
      {r.items.length === 0 ? (
        <StateView
          kind="empty"
          title={`No scanned project requires ${r.runtime}`}
          description="Projects appear here after a repository scan, when their manifests declare or imply this runtime."
          compact
        />
      ) : (
        <ul className="flex flex-col gap-1.5">
          {r.items.map((i) => (
            <li key={i.project.id} className="rounded-md border border-line px-2.5 py-2">
              <div className="flex items-center justify-between gap-2">
                <button
                  className="truncate text-left text-[12.5px] font-medium text-fg underline-offset-2 hover:underline disabled:no-underline"
                  onClick={() => onSelectProject?.(i.project.id)}
                  disabled={!onSelectProject}
                >
                  {i.project.name}
                </button>
                <Badge tone={OUTCOME[i.outcome].tone}>{OUTCOME[i.outcome].label}</Badge>
              </div>
              <div className="mt-1 text-[11.5px] leading-5 text-muted">{i.explanation}</div>
              <div className="mt-1 font-mono text-[11px] text-faint">{i.requirements.join(" · ")}</div>
            </li>
          ))}
        </ul>
      )}
      <ul className="list-disc pl-4 text-[11px] leading-5 text-faint">
        {r.limitations.map((l) => (
          <li key={l}>{l}</li>
        ))}
      </ul>
    </div>
  );
}
