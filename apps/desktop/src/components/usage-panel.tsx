import type { DayUsage, SessionUsage, TokenCounts, UsageReport } from "@agasthy/shared-types";
import { Badge, StateView, cn } from "@agasthy/ui";
import { Coins } from "lucide-react";
import { useState } from "react";
import { formatTokens, formatUsd, shortenHome } from "../lib/format";
import { Panel } from "./page";

const AGENT_NAME: Record<string, string> = { "claude-code": "Claude Code", codex: "Codex", "copilot-cli": "GitHub Copilot CLI", hermes: "Hermes Agent" };

export const totalTokens = (t: TokenCounts) => t.input + t.output + t.cache_write + t.cache_read;

function add(a: TokenCounts, b: TokenCounts): TokenCounts {
  return { input: a.input + b.input, output: a.output + b.output, cache_write: a.cache_write + b.cache_write, cache_read: a.cache_read + b.cache_read, requests: a.requests + b.requests };
}
const ZERO: TokenCounts = { input: 0, output: 0, cache_write: 0, cache_read: 0, requests: 0 };

interface Agg {
  tokens: TokenCounts;
  cost: number;
  unpriced: boolean;
}
function aggregate(days: DayUsage[]): Agg {
  return days.reduce<Agg>(
    (a, d) => ({ tokens: add(a.tokens, d.tokens), cost: a.cost + (d.estimated_cost_usd ?? 0), unpriced: a.unpriced || d.estimated_cost_usd == null }),
    { tokens: ZERO, cost: 0, unpriced: false },
  );
}

function isoDay(d: Date): string {
  return d.toISOString().slice(0, 10);
}

/** The usage log entry that belongs to a running agent session, matched by agent, folder and time. */
export function matchSession(usage: UsageReport | undefined, agent: string, cwd: string | null, startedSecs: number): SessionUsage | null {
  if (!usage || !cwd) return null;
  const startedMs = startedSecs * 1000;
  const c = usage.sessions.filter((s) => s.agent === agent && s.cwd === cwd && (s.last_at ?? 0) >= startedMs - 60_000);
  c.sort((a, b) => (b.last_at ?? 0) - (a.last_at ?? 0));
  return c[0] ?? null;
}

export function UsagePanel({ report, loading, error }: { report?: UsageReport; loading: boolean; error: string | null }) {
  const [hover, setHover] = useState<number | null>(null);
  if (loading) {
    return (
      <Panel title="Token usage">
        <StateView kind="loading" title="Reading agents' usage logs" compact />
      </Panel>
    );
  }
  if (error || !report) {
    return (
      <Panel title="Token usage">
        <StateView kind="error" title={error ?? "Usage could not be read"} compact />
      </Panel>
    );
  }
  const found = report.sources.filter((s) => s.found);
  if (report.days.length === 0) {
    return (
      <Panel title="Token usage">
        <div className="p-1 text-[12.5px] text-muted">
          No agent usage in the last {report.window_days} days.{" "}
          {found.length ? `Looked in ${found.map((s) => shortenHome(s.path)).join(", ")}.` : "No Claude Code, Codex, GitHub Copilot CLI or Hermes logs were found. Copilot in VS Code is not read yet."}
        </div>
      </Panel>
    );
  }

  const today = new Date();
  const dayKeys = Array.from({ length: report.window_days }, (_, i) => {
    const d = new Date(today);
    d.setUTCDate(d.getUTCDate() - (report.window_days - 1 - i));
    return isoDay(d);
  });
  const byDay = new Map<string, DayUsage[]>();
  for (const d of report.days) byDay.set(d.day, [...(byDay.get(d.day) ?? []), d]);
  const series = dayKeys.map((k) => ({ day: k, ...aggregate(byDay.get(k) ?? []) }));
  const max = Math.max(1, ...series.map((s) => totalTokens(s.tokens)));
  const sumLast = (n: number) => aggregate(dayKeys.slice(-n).flatMap((k) => byDay.get(k) ?? []));
  const windows: [string, Agg][] = [
    ["Today (UTC)", sumLast(1)],
    ["Last 7 days", sumLast(7)],
    [`Last ${report.window_days} days`, sumLast(report.window_days)],
  ];

  const group = (key: (d: DayUsage) => string) => {
    const m = new Map<string, DayUsage[]>();
    for (const d of report.days) m.set(key(d), [...(m.get(key(d)) ?? []), d]);
    return [...m.entries()].map(([k, v]) => ({ key: k, ...aggregate(v) })).sort((a, b) => totalTokens(b.tokens) - totalTokens(a.tokens));
  };
  const projects = group((d) => d.cwd ?? "unknown folder").slice(0, 8);
  const models = group((d) => d.model).slice(0, 8);
  const agents = group((d) => AGENT_NAME[d.agent] ?? d.agent);
  const h = hover == null ? null : series[hover];

  return (
    <Panel
      title={
        <span className="flex items-center gap-1.5">
          <Coins className="size-3.5" aria-hidden /> Token usage
        </span>
      }
      actions={<span className="text-[11px] text-faint">from {agents.map((a) => a.key).join(", ")} logs · counters only, no message text</span>}
    >
      <div className="flex flex-col gap-4">
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
          {windows.map(([label, a]) => (
            <div key={label} className="rounded-md border border-line px-3 py-2.5">
              <div className="text-[11.5px] text-muted">{label}</div>
              <div className="tabular mt-0.5 text-[19px] font-semibold text-fg">{formatTokens(totalTokens(a.tokens))} tokens</div>
              <div className="tabular text-[11.5px] text-muted">
                {formatTokens(a.tokens.output)} output · {a.tokens.requests.toLocaleString()} requests · ≈{formatUsd(a.cost)}
                {a.unpriced ? "+" : ""}
              </div>
            </div>
          ))}
        </div>

        <figure className="m-0">
          <figcaption className="mb-1.5 flex items-baseline justify-between text-[11.5px] text-muted">
            <span>Tokens per day, all agents</span>
            <span className="tabular text-fg">{h ? `${h.day}: ${formatTokens(totalTokens(h.tokens))} tokens · ${h.tokens.requests} requests · ≈${formatUsd(h.cost)}` : " "}</span>
          </figcaption>
          <div className="relative flex h-28 items-end gap-[2px] border-b border-line" role="img" aria-label={`Daily token usage over the last ${report.window_days} days`} onMouseLeave={() => setHover(null)}>
            {series.map((s, i) => {
              const v = totalTokens(s.tokens);
              return (
                <button
                  key={s.day}
                  type="button"
                  className="group flex h-full flex-1 items-end focus:outline-none"
                  onMouseEnter={() => setHover(i)}
                  onFocus={() => setHover(i)}
                  aria-label={`${s.day}: ${formatTokens(v)} tokens`}
                >
                  <span
                    className={cn("block w-full rounded-t-[4px] bg-accent/70 transition-colors group-hover:bg-accent group-focus-visible:bg-accent", v === 0 && "bg-transparent")}
                    style={{ height: v === 0 ? 0 : `${Math.max(2, (v / max) * 100)}%` }}
                  />
                </button>
              );
            })}
          </div>
          <div className="mt-1 flex justify-between text-[10.5px] text-faint">
            <span>{dayKeys[0]}</span>
            <span>today</span>
          </div>
        </figure>

        <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
          <Breakdown title="By project folder" rows={projects.map((p) => ({ ...p, key: shortenHome(p.key) }))} />
          <Breakdown title="By model" rows={models} />
        </div>
        <p className="text-[11px] text-faint">
          Tokens include prompt-cache reads, which dominate long agent sessions. Cost is an estimate at API list prices; subscription plans (Claude Max, ChatGPT Pro) are
          billed differently. “+” means some models have no price data. Days are UTC.
        </p>
      </div>
    </Panel>
  );
}

function Breakdown({ title, rows }: { title: string; rows: { key: string; tokens: TokenCounts; cost: number; unpriced: boolean }[] }) {
  const max = Math.max(1, ...rows.map((r) => totalTokens(r.tokens)));
  return (
    <table className="h-fit w-full self-start text-[12px]">
      <caption className="mb-1 text-left text-[11.5px] text-muted">{title}</caption>
      <thead className="sr-only">
        <tr>
          <th>Name</th>
          <th>Tokens</th>
          <th>Estimated cost</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((r) => (
          <tr key={r.key} className="border-b border-line/50 last:border-0">
            <td className="max-w-0 py-1.5 pr-2">
              <div className="truncate text-fg" title={r.key}>
                {r.key}
              </div>
              <div className="mt-1 h-1 rounded-full bg-raised">
                <div className="h-full rounded-full bg-accent/60" style={{ width: `${(totalTokens(r.tokens) / max) * 100}%` }} />
              </div>
            </td>
            <td className="tabular w-20 py-1.5 text-right text-muted">{formatTokens(totalTokens(r.tokens))}</td>
            <td className="tabular w-20 py-1.5 text-right text-fg">
              {r.unpriced && r.cost === 0 ? <Badge>no price</Badge> : `≈${formatUsd(r.cost)}${r.unpriced ? "+" : ""}`}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
