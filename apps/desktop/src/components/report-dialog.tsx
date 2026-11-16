import { Button, StateView } from "@agasthy/ui";
import { useQuery } from "@tanstack/react-query";
import { Bug, Check, Copy, ExternalLink, X } from "lucide-react";
import { useState } from "react";
import { api, errorMessage } from "../lib/ipc";

/**
 * "Report a problem": a redacted summary of this install the user reviews and
 * edits, then copies or opens as a pre-filled GitHub issue. Nothing is sent
 * from here; the browser opens GitHub with the text the user sees.
 */
export function ReportDialog({ onClose, context }: { onClose: () => void; context?: string }) {
  const q = useQuery({ queryKey: ["diagnostics-report"], queryFn: api.diagnosticsReport, staleTime: 0, gcTime: 0 });
  const [title, setTitle] = useState("");
  const [what, setWhat] = useState(context ? `AGASTHY showed this error:\n\n${context}\n` : "");
  const [edited, setEdited] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const report = edited ?? q.data ?? "";

  const body = () =>
    `## What happened\n\n${what.trim() || "_Describe what you did and what you expected._"}\n\n<details><summary>AGASTHY report</summary>\n\n${report}\n</details>\n`;

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(body());
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      setCopied(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-6" role="dialog" aria-modal="true" aria-label="Report a problem">
      <div className="flex max-h-[88vh] w-full max-w-2xl flex-col overflow-hidden rounded-lg border border-line bg-surface shadow-2xl">
        <div className="flex h-11 shrink-0 items-center justify-between border-b border-line px-4">
          <div className="flex items-center gap-2 text-[13px] font-medium text-fg">
            <Bug className="size-4 text-accent" aria-hidden /> Report a problem
          </div>
          <Button size="icon" variant="ghost" aria-label="Close" onClick={onClose}>
            <X />
          </Button>
        </div>
        <div className="flex min-h-0 flex-1 flex-col gap-3 overflow-y-auto p-4 text-[12.5px]">
          <label className="flex flex-col gap-1">
            <span className="text-muted">Title</span>
            <input
              className="h-8 rounded-md border border-line bg-surface px-2.5 text-fg focus:outline-none focus:ring-2 focus:ring-accent/50"
              placeholder="Short summary, e.g. “Agents page shows no sessions”"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              maxLength={200}
            />
          </label>
          <label className="flex flex-col gap-1">
            <span className="text-muted">What happened</span>
            <textarea
              className="min-h-[72px] rounded-md border border-line bg-surface p-2.5 text-fg focus:outline-none focus:ring-2 focus:ring-accent/50"
              placeholder="What you did, what you expected, what happened instead."
              value={what}
              onChange={(e) => setWhat(e.target.value)}
            />
          </label>
          <div className="flex flex-col gap-1">
            <span className="text-muted">Attached report (edit or delete anything you don't want to share)</span>
            {q.isLoading ? (
              <StateView kind="loading" title="Preparing the report" compact />
            ) : q.isError ? (
              <StateView kind="error" title={errorMessage(q.error)} compact />
            ) : (
              <textarea
                aria-label="Attached report"
                className="min-h-[220px] rounded-md border border-line bg-raised p-2.5 font-mono text-[11.5px] leading-relaxed text-fg focus:outline-none focus:ring-2 focus:ring-accent/50"
                value={report}
                onChange={(e) => setEdited(e.target.value)}
                spellCheck={false}
              />
            )}
          </div>
          <p className="text-[11.5px] text-faint">
            Includes version, macOS and chip, scan status, counts, settings and the last lines of the local diagnostics log. Your home folder is shown as ~; no
            hostname, project names or file contents. GitHub issues are public.
          </p>
        </div>
        <div className="flex shrink-0 items-center justify-end gap-2 border-t border-line px-4 py-3">
          <Button size="sm" variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button size="sm" onClick={() => void copy()} disabled={!report}>
            {copied ? <Check aria-hidden /> : <Copy aria-hidden />} {copied ? "Copied" : "Copy"}
          </Button>
          <Button size="sm" variant="primary" disabled={!report} onClick={() => void api.openIssue(title, body())}>
            <ExternalLink aria-hidden /> Open GitHub issue
          </Button>
        </div>
      </div>
    </div>
  );
}
