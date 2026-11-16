import { Button } from "@agasthy/ui";
import { type ErrorComponentProps, useNavigate } from "@tanstack/react-router";
import { AlertOctagon } from "lucide-react";
import { useState } from "react";
import { ReportDialog } from "./report-dialog";

/**
 * Shown in place of a page that failed to render. The sidebar and every other
 * page keep working; nothing about the machine was changed by the failure.
 */
export function PageError({ error, reset }: ErrorComponentProps) {
  const navigate = useNavigate();
  const message = error instanceof Error ? error.message : String(error);
  const [report, setReport] = useState(false);
  return (
    <div className="flex h-full items-center justify-center p-8" role="alert">
      <div className="max-w-lg rounded-lg border border-line bg-surface p-5">
        <div className="flex items-center gap-2 text-[14px] font-semibold text-fg">
          <AlertOctagon className="size-4 text-danger" aria-hidden /> This page could not be shown
        </div>
        <p className="mt-2 text-[12.5px] text-muted">
          AGASTHY hit an unexpected error while drawing this page. Nothing on your machine was changed. Other pages still work.
        </p>
        <pre className="mt-3 max-h-32 overflow-auto rounded-md bg-raised p-2.5 font-mono text-[11.5px] text-muted">{message}</pre>
        <div className="mt-4 flex gap-2">
          <Button size="sm" variant="primary" onClick={reset}>
            Try again
          </Button>
          <Button
            size="sm"
            variant="ghost"
            onClick={() => {
              reset();
              void navigate({ to: "/" });
            }}
          >
            Go to Overview
          </Button>
          <Button size="sm" variant="ghost" onClick={() => setReport(true)}>
            Report this problem
          </Button>
        </div>
        {report ? <ReportDialog context={message} onClose={() => setReport(false)} /> : null}
      </div>
    </div>
  );
}
