import type { ActionPlan, ActionRequest, ActionResult } from "@agasthy/shared-types";
import { Badge, Button, StateView } from "@agasthy/ui";
import { useQueryClient } from "@tanstack/react-query";
import { AlertTriangle, CheckCircle2, ShieldCheck, Undo2, X, XCircle } from "lucide-react";
import { useEffect, useState } from "react";
import { formatBytes, shortenHome } from "../lib/format";
import { api, errorMessage } from "../lib/ipc";

type Phase = { step: "planning" } | { step: "review"; plan: ActionPlan } | { step: "running"; plan: ActionPlan } | { step: "done"; plan: ActionPlan; result: ActionResult } | { step: "error"; message: string };

/**
 * Every change AGASTHY makes goes through this dialog:
 * plan (dry run) → review exact targets and risks → explicit confirm → execute → verified result.
 */
export function ActionDialog({ request, onClose }: { request: ActionRequest; onClose: (changed: boolean) => void }) {
  const [phase, setPhase] = useState<Phase>({ step: "planning" });
  const qc = useQueryClient();

  useEffect(() => {
    let live = true;
    api
      .planAction(request)
      .then((plan) => live && setPhase({ step: "review", plan }))
      .catch((e) => live && setPhase({ step: "error", message: errorMessage(e) }));
    return () => {
      live = false;
    };
  }, [request]);

  const confirm = async (plan: ActionPlan) => {
    if (!plan.token) return;
    setPhase({ step: "running", plan });
    try {
      const result = await api.executeAction(plan.token);
      setPhase({ step: "done", plan, result });
      await Promise.all([
        qc.invalidateQueries({ queryKey: ["processes"] }),
        qc.invalidateQueries({ queryKey: ["quarantine"] }),
        qc.invalidateQueries({ queryKey: ["disk-analysis"] }),
        qc.invalidateQueries({ queryKey: ["audit"] }),
      ]);
    } catch (e) {
      setPhase({ step: "error", message: errorMessage(e) });
    }
  };

  const changed = phase.step === "done" && phase.result.outcome !== "failed";
  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-6" role="dialog" aria-modal="true" aria-label="Review action">
      <div className="flex max-h-[85vh] w-full max-w-2xl flex-col overflow-hidden rounded-lg border border-line bg-surface shadow-2xl">
        <div className="flex h-11 shrink-0 items-center justify-between border-b border-line px-4">
          <div className="flex items-center gap-2 text-[13px] font-medium text-fg">
            <ShieldCheck className="size-4 text-accent" aria-hidden />
            {phase.step === "review" || phase.step === "running" || phase.step === "done" ? phase.plan.title : "Preparing a dry run"}
          </div>
          <Button size="icon" variant="ghost" aria-label="Close" onClick={() => onClose(changed)} disabled={phase.step === "running"}>
            <X />
          </Button>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto p-4">
          {phase.step === "planning" ? <StateView kind="loading" title="Checking every target" compact /> : null}
          {phase.step === "error" ? <StateView kind="error" title="Could not prepare the action" description={phase.message} compact /> : null}
          {phase.step === "review" || phase.step === "running" ? <PlanView plan={phase.plan} /> : null}
          {phase.step === "done" ? <ResultView result={phase.result} /> : null}
        </div>
        <div className="flex shrink-0 items-center justify-between gap-2 border-t border-line px-4 py-3">
          <span className="text-[11.5px] text-faint">
            {phase.step === "review"
              ? phase.plan.can_execute
                ? `Dry run only so far. Nothing has changed. Confirmation expires in ${Math.round(phase.plan.expires_in_secs / 60)} min.`
                : "This cannot run as planned."
              : phase.step === "done"
                ? "Recorded in the audit log."
                : ""}
          </span>
          <div className="flex gap-2">
            <Button variant="ghost" onClick={() => onClose(changed)} disabled={phase.step === "running"}>
              {phase.step === "done" ? "Close" : "Cancel"}
            </Button>
            {phase.step === "review" || phase.step === "running" ? (
              <Button
                variant={phase.plan.reversible ? "primary" : "danger"}
                disabled={!phase.plan.can_execute || phase.step === "running"}
                onClick={() => void confirm(phase.plan)}
              >
                {phase.step === "running" ? "Working…" : phase.plan.reversible ? "Confirm" : "Confirm (cannot be undone)"}
              </Button>
            ) : null}
          </div>
        </div>
      </div>
    </div>
  );
}

function PlanView({ plan }: { plan: ActionPlan }) {
  return (
    <div className="flex flex-col gap-3">
      <p className="text-[12.5px] leading-5 text-muted">{plan.summary}</p>
      <ul className="flex flex-col divide-y divide-line rounded-md border border-line">
        {plan.items.map((i) => (
          <li key={i.label} className="flex items-start justify-between gap-3 px-3 py-2">
            <div className="min-w-0">
              <div className="selectable break-all font-mono text-[11.5px] text-fg">{shortenHome(i.label)}</div>
              {i.detail ? <div className="text-[11px] text-faint">{i.detail.replaceAll("_", " ")}</div> : null}
              {i.blocked ? <div className="mt-0.5 text-[11.5px] text-danger">Blocked: {i.blocked}</div> : null}
            </div>
            {i.bytes != null ? <span className="tabular shrink-0 text-[12px] text-fg">{formatBytes(i.bytes)}</span> : null}
          </li>
        ))}
      </ul>
      {plan.total_bytes != null ? (
        <div className="flex justify-between text-[12.5px]">
          <span className="text-muted">Total</span>
          <span className="tabular font-medium text-fg">{formatBytes(plan.total_bytes)}</span>
        </div>
      ) : null}
      {plan.warnings.map((w) => (
        <div key={w} className="flex gap-2 rounded-md bg-warning/10 px-2.5 py-2 text-[12px] text-fg">
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0 text-warning" aria-hidden />
          {w}
        </div>
      ))}
      <div className="flex gap-2 text-[12px] text-muted">
        <Undo2 className="mt-0.5 size-3.5 shrink-0" aria-hidden />
        <span>
          <Badge tone={plan.reversible ? "success" : "danger"}>{plan.reversible ? "reversible" : "not reversible"}</Badge> {plan.recovery}
        </span>
      </div>
    </div>
  );
}

function ResultView({ result }: { result: ActionResult }) {
  const ok = result.outcome === "succeeded";
  const Icon = ok ? CheckCircle2 : result.outcome === "partially_succeeded" ? AlertTriangle : XCircle;
  return (
    <div className="flex flex-col gap-3" role="status">
      <div className="flex items-center gap-2">
        <Icon className={ok ? "size-5 text-success" : result.outcome === "failed" ? "size-5 text-danger" : "size-5 text-warning"} aria-hidden />
        <span className="text-[14px] font-medium text-fg">{result.title}</span>
      </div>
      <p className="text-[12.5px] leading-5 text-muted">{result.message}</p>
      {result.bytes != null ? <div className="text-[12.5px] text-fg">{formatBytes(result.bytes)}</div> : null}
      {result.details.length ? (
        <ul className="list-disc pl-4 text-[12px] text-muted">
          {result.details.map((d) => (
            <li key={d} className="selectable break-all">
              {shortenHome(d)}
            </li>
          ))}
        </ul>
      ) : null}
      <div className="text-[11.5px] text-faint">Verification: {result.verification}</div>
      {result.recovery ? <div className="text-[11.5px] text-faint">Recovery: {result.recovery}</div> : null}
    </div>
  );
}
