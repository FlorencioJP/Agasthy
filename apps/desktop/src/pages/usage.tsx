import type { SessionUsage } from "@agasthy/shared-types";
import { Badge, Button, StateView, cn } from "@agasthy/ui";
import { Download } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { PageHeader, Panel } from "../components/page";
import { UsagePanel, totalTokens } from "../components/usage-panel";
import { formatRelative, formatTokens, formatUsd, shortenHome } from "../lib/format";
import { api, errorMessage } from "../lib/ipc";

const RANGES = [7, 30, 90] as const;
const AGENT_NAME: Record<string, string> = { "claude-code": "Claude Code", codex: "Codex", "copilot-cli": "GitHub Copilot CLI", hermes: "Hermes Agent" };

export function UsagePage() {
  const [days, setDays] = useState<(typeof RANGES)[number]>(30);
  const q = useQuery({ queryKey: ["agent-usage", days], queryFn: () => api.agentUsage(days), refetchInterval: 30_000 });
  const r = q.data;
  const [saved, setSaved] = useState<string | null>(null);
  const [exporting, setExporting] = useState(false);
  const exportCsv = async () => {
    setExporting(true);
    try {
      const path = await api.exportUsageCsv(days);
      if (path) setSaved(`Saved to ${path}`);
    } catch (e) {
      setSaved(`Export failed: ${errorMessage(e)}`);
    } finally {
      setExporting(false);
    }
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title="Token Usage"
        subtitle="what your coding agents consumed, read from their own local logs"
        actions={
          <div className="flex items-center gap-2">
          {saved ? <span className="max-w-[260px] truncate text-[11.5px] text-muted" role="status">{saved}</span> : null}
          <Button size="sm" variant="ghost" disabled={!r || r.days.length === 0 || exporting} onClick={() => void exportCsv()}>
            <Download aria-hidden /> {exporting ? "Exporting…" : "Export CSV"}
          </Button>
          <div className="flex items-center gap-1 rounded-md border border-line p-0.5" role="radiogroup" aria-label="Time range">
            {RANGES.map((d) => (
              <button
                key={d}
                role="radio"
                aria-checked={days === d}
                onClick={() => setDays(d)}
                className={cn("h-6 rounded px-2 text-[12px]", days === d ? "bg-raised text-fg" : "text-muted hover:text-fg")}
              >
                {d} days
              </button>
            ))}
          </div>
          </div>
        }
      />
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="flex flex-col gap-4 p-5">
          <UsagePanel report={r} loading={q.isLoading} error={q.isError ? errorMessage(q.error) : null} />
          {r ? <SessionsTable sessions={r.sessions} /> : null}
          {r ? (
            <Panel title="Sources">
              <ul className="flex flex-col gap-1.5 text-[12.5px]">
                {r.sources.map((s) => (
                  <li key={s.path} className="flex items-center gap-2">
                    <Badge tone={s.found ? "success" : "neutral"}>{s.found ? "found" : "not installed"}</Badge>
                    <span className="text-fg">{AGENT_NAME[s.agent] ?? s.agent}</span>
                    <code className="truncate font-mono text-[11.5px] text-muted">{shortenHome(s.path)}</code>
                  </li>
                ))}
                {r.problems.map((p) => (
                  <li key={p} className="text-danger">
                    {p}
                  </li>
                ))}
              </ul>
              <p className="mt-3 text-[11.5px] text-faint">
                Only token counters, model names, timestamps and the session folder are read. Prompts, replies and tool output are never stored or shown. Agents
                that keep no local usage records (Cursor, Gemini CLI, OpenClaw, …) appear on the Agents page but not here.
              </p>
            </Panel>
          ) : null}
        </div>
      </div>
    </div>
  );
}

function SessionsTable({ sessions }: { sessions: SessionUsage[] }) {
  const [all, setAll] = useState(false);
  if (sessions.length === 0) return null;
  const rows = all ? sessions : sessions.slice(0, 15);
  return (
    <Panel title={`Sessions (${sessions.length})`} bodyClassName="p-0">
      {sessions.length === 0 ? (
        <StateView kind="empty" title="No sessions" compact />
      ) : (
        <table className="w-full text-[12.5px]">
          <thead className="text-left text-[11px] text-muted">
            <tr className="border-b border-line">
              <th className="px-3.5 py-2 font-medium">Agent · folder</th>
              <th className="px-2 py-2 font-medium">Model</th>
              <th className="px-2 py-2 font-medium">Last active</th>
              <th className="px-2 py-2 text-right font-medium">Requests</th>
              <th className="px-2 py-2 text-right font-medium">Output</th>
              <th className="px-2 py-2 text-right font-medium">Tokens</th>
              <th className="px-3.5 py-2 text-right font-medium">≈ Cost</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((s) => (
              <tr key={`${s.agent}:${s.session_id}`} className="border-b border-line/50 last:border-0">
                <td className="max-w-0 px-3.5 py-2">
                  <div className="truncate text-fg">{s.cwd ? shortenHome(s.cwd) : "unknown folder"}</div>
                  <div className="text-[11px] text-faint">{AGENT_NAME[s.agent] ?? s.agent}</div>
                </td>
                <td className="max-w-[180px] truncate px-2 py-2 text-muted">{s.models.map((m) => m.model).join(", ")}</td>
                <td className="whitespace-nowrap px-2 py-2 text-muted">{s.last_at ? formatRelative(s.last_at) : "—"}</td>
                <td className="tabular px-2 py-2 text-right text-muted">{s.tokens.requests.toLocaleString()}</td>
                <td className="tabular px-2 py-2 text-right text-muted">{formatTokens(s.tokens.output)}</td>
                <td className="tabular px-2 py-2 text-right text-fg">{formatTokens(totalTokens(s.tokens))}</td>
                <td className="tabular px-3.5 py-2 text-right text-fg">
                  {s.estimated_cost_usd == null ? <span className="text-faint">no price</span> : `${formatUsd(s.estimated_cost_usd)}${s.cost_partial ? "+" : ""}`}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      )}
      {sessions.length > 15 ? (
        <div className="border-t border-line px-3.5 py-2">
          <button className="text-[12px] text-accent hover:underline" onClick={() => setAll((a) => !a)}>
            {all ? "Show fewer" : `Show all ${sessions.length}`}
          </button>
        </div>
      ) : null}
    </Panel>
  );
}
