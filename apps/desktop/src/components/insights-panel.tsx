import type { ActionRequest, Insight, InsightSeverity } from "@agasthy/shared-types";
import { Badge, Button, StateView, cn } from "@agasthy/ui";
import { useQuery } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { AlertOctagon, AlertTriangle, CheckCircle2, Info } from "lucide-react";
import { useState } from "react";
import { api, errorMessage } from "../lib/ipc";
import { ActionDialog } from "./action-dialog";
import { Panel } from "./page";
import { useScanActivity } from "./scan-activity";

const SEVERITY: Record<InsightSeverity, { icon: typeof Info; tone: string; label: string }> = {
  critical: { icon: AlertOctagon, tone: "text-danger", label: "Critical" },
  warning: { icon: AlertTriangle, tone: "text-warning", label: "Needs attention" },
  info: { icon: Info, tone: "text-info", label: "Info" },
};

/** Ranked, actionable findings across the app. Actions open a page, start a read-only scan, or open a dry-run review. */
export function InsightsPanel() {
  const q = useQuery({ queryKey: ["insights"], queryFn: api.insights, refetchInterval: 60_000 });
  const navigate = useNavigate();
  const activity = useScanActivity();
  const [review, setReview] = useState<ActionRequest | null>(null);

  const act = (i: Insight) => {
    const a = i.action;
    if (!a) return;
    if (a.kind === "open") void navigate({ to: a.route });
    else if (a.kind === "review") setReview(a.request);
    else void activity.start(a.scan).then(() => q.refetch());
  };

  return (
    <Panel title="Insights" actions={<span className="text-[11px] text-faint">ranked from live and scanned data · updates every minute</span>} bodyClassName="p-0">
      {q.isLoading ? (
        <StateView kind="loading" title="Looking for anything that needs attention" compact />
      ) : q.isError || !q.data ? (
        <StateView kind="error" title={errorMessage(q.error)} compact />
      ) : q.data.insights.length === 0 ? (
        <div className="flex items-center gap-2 px-3.5 py-3 text-[12.5px] text-muted">
          <CheckCircle2 className="size-4 text-success" aria-hidden /> Nothing needs attention right now.
        </div>
      ) : (
        <ul className="divide-y divide-line/70" aria-label="Insights">
          {q.data.insights.map((i) => {
            const s = SEVERITY[i.severity];
            const Icon = s.icon;
            const busy = i.action?.kind === "scan" && activity.running.has(i.action.scan);
            return (
              <li key={i.id} className="flex items-start gap-3 px-3.5 py-3">
                <Icon className={cn("mt-0.5 size-4 shrink-0", s.tone)} aria-label={s.label} />
                <div className="min-w-0 flex-1">
                  <div className="flex items-center gap-2">
                    <span className="text-[13px] font-medium text-fg">{i.title}</span>
                    <Badge>{i.area}</Badge>
                  </div>
                  <div className="mt-0.5 text-[12px] text-muted">{i.detail}</div>
                </div>
                {i.action ? (
                  <Button size="sm" variant={i.severity === "info" ? "ghost" : "secondary"} disabled={busy} onClick={() => act(i)}>
                    {busy ? "Scanning…" : i.action.label}
                  </Button>
                ) : null}
              </li>
            );
          })}
        </ul>
      )}
      {review ? (
        <ActionDialog
          request={review}
          onClose={() => {
            setReview(null);
            void q.refetch();
          }}
        />
      ) : null}
    </Panel>
  );
}
