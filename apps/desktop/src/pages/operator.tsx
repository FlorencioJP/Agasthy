import type { ActionRequest, ChatTurn, OperatorAnswer, Proposal } from "@agasthy/shared-types";
import { Badge, Button, cn } from "@agasthy/ui";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { ChevronRight, Cloud, Cpu, Loader2, SendHorizontal, ShieldCheck, Wrench } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { ActionDialog } from "../components/action-dialog";
import { PageHeader } from "../components/page";
import { api, errorMessage } from "../lib/ipc";

type Message =
  | { role: "user"; text: string }
  | { role: "assistant"; answer: OperatorAnswer }
  | { role: "error"; text: string };

const TOOL_LABEL: Record<string, string> = {
  machine_overview: "Machine overview",
  top_processes: "Processes",
  listening_ports: "Listening ports",
  search: "Search",
  entity_details: "Item details",
  repositories: "Repositories",
  disk_cleanup_candidates: "Disk analysis",
  removal_impact: "Impact simulation",
  ai_ecosystem: "AI ecosystem",
  security_findings: "Security findings",
  docker_status: "Docker usage",
  propose_action: "Proposal (dry run)",
};

export function OperatorPage() {
  const settings = useQuery({ queryKey: ["operator-settings"], queryFn: api.operatorSettings });
  const [messages, setMessages] = useState<Message[]>([]);
  const [input, setInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [modelChoice, setModelChoice] = useState<boolean | null>(null);
  const [review, setReview] = useState<ActionRequest | null>(null);
  const endRef = useRef<HTMLDivElement>(null);

  const s = settings.data;
  const provider = s?.provider ?? null;
  const cloudBlocked = provider !== null && !s?.provider_is_local && !s?.cloud_consent;
  const needsKey = provider !== null && (provider.kind === "anthropic" || Boolean(s?.presets.find((p) => p.id === provider.preset)?.needs_key));
  const keyMissing = needsKey && !s?.has_key;
  const modelReady = provider !== null && !cloudBlocked && !keyMissing;
  const useModel = modelChoice ?? modelReady;

  useEffect(() => {
    endRef.current?.scrollIntoView({ block: "end" });
  }, [messages, busy]);

  const ask = async (question: string) => {
    const q = question.trim();
    if (!q || busy) return;
    const history: ChatTurn[] = messages.flatMap((m): ChatTurn[] =>
      m.role === "user" ? [{ role: "user", content: m.text }] : m.role === "assistant" ? [{ role: "assistant", content: m.answer.answer }] : [],
    );
    setMessages((ms) => [...ms, { role: "user", text: q }]);
    setInput("");
    setBusy(true);
    try {
      const answer = await api.operatorAsk(q, history, useModel && modelReady);
      setMessages((ms) => [...ms, { role: "assistant", answer }]);
    } catch (e) {
      setMessages((ms) => [...ms, { role: "error", text: errorMessage(e) }]);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex h-full min-h-0 flex-col">
      <PageHeader
        title="AI Operator"
        subtitle="answers from your machine's data · proposes changes, never makes them"
        actions={
          <div className="flex items-center gap-1 rounded-md border border-line p-0.5" role="radiogroup" aria-label="Who answers">
            <ModeButton active={!useModel || !modelReady} onClick={() => setModelChoice(false)} icon={<ShieldCheck aria-hidden />} label="Built-in" />
            <ModeButton
              active={useModel && modelReady}
              disabled={!modelReady}
              onClick={() => setModelChoice(true)}
              icon={s?.provider_is_local ? <Cpu aria-hidden /> : <Cloud aria-hidden />}
              label={provider ? provider.model : "Model"}
              title={!provider ? "Set up a model in Settings → AI model" : cloudBlocked ? "Cloud models are off in Settings → AI model" : keyMissing ? "Add an API key in Settings → AI model" : undefined}
            />
          </div>
        }
      />
      <div className="min-h-0 flex-1 overflow-y-auto">
        <div className="mx-auto flex max-w-3xl flex-col gap-4 p-5">
          <ModeNotice useModel={useModel && modelReady} local={Boolean(s?.provider_is_local)} configured={provider !== null} host={provider?.base_url} />
          {messages.length === 0 ? (
            <div className="space-y-2">
              <div className="text-[12px] text-muted">Try asking</div>
              <div className="flex flex-wrap gap-2">
                {(s?.suggestions ?? []).map((q) => (
                  <button
                    key={q}
                    onClick={() => void ask(q)}
                    className="rounded-full border border-line bg-surface px-3 py-1.5 text-[12.5px] text-fg transition-colors hover:bg-hover"
                  >
                    {q}
                  </button>
                ))}
              </div>
            </div>
          ) : null}
          {messages.map((m, i) =>
            m.role === "user" ? (
              <div key={i} className="self-end rounded-lg bg-accent/15 px-3.5 py-2 text-[13px] text-fg">
                {m.text}
              </div>
            ) : m.role === "error" ? (
              <div key={i} role="alert" className="rounded-lg border border-danger/30 bg-danger/10 px-3.5 py-2.5 text-[12.5px] text-danger">
                {m.text}
              </div>
            ) : (
              <AnswerView key={i} answer={m.answer} onReview={(p) => setReview(p.request as unknown as ActionRequest)} />
            ),
          )}
          {busy ? (
            <div className="flex items-center gap-2 text-[12.5px] text-muted" role="status">
              <Loader2 className="size-3.5 animate-spin" aria-hidden /> Looking at your machine…
            </div>
          ) : null}
          <div ref={endRef} />
        </div>
      </div>
      <form
        className="border-t border-line bg-bg p-3"
        onSubmit={(e) => {
          e.preventDefault();
          void ask(input);
        }}
      >
        <div className="mx-auto flex max-w-3xl items-center gap-2">
          <input
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder="Ask about processes, disk space, repositories, AI tools…"
            aria-label="Ask the Operator"
            maxLength={4000}
            className="h-9 min-w-0 flex-1 rounded-md border border-line bg-surface px-3 text-[13px] text-fg placeholder:text-faint focus:outline-none focus:ring-2 focus:ring-accent/50"
          />
          <Button type="submit" variant="primary" disabled={busy || !input.trim()} aria-label="Send">
            <SendHorizontal aria-hidden /> Ask
          </Button>
        </div>
      </form>
      {review ? <ActionDialog request={review} onClose={() => setReview(null)} /> : null}
    </div>
  );
}

function ModeButton({
  active,
  disabled,
  onClick,
  icon,
  label,
  title,
}: {
  active: boolean;
  disabled?: boolean;
  onClick: () => void;
  icon: React.ReactNode;
  label: string;
  title?: string;
}) {
  return (
    <button
      role="radio"
      aria-checked={active}
      disabled={disabled}
      title={title}
      onClick={onClick}
      className={cn(
        "flex h-6 items-center gap-1.5 rounded px-2 text-[12px] [&_svg]:size-3.5",
        active ? "bg-raised text-fg" : "text-muted hover:text-fg",
        disabled && "cursor-not-allowed opacity-50 hover:text-muted",
      )}
    >
      {icon}
      <span className="max-w-[140px] truncate">{label}</span>
    </button>
  );
}

function ModeNotice({ useModel, local, configured, host }: { useModel: boolean; local: boolean; configured: boolean; host?: string }) {
  if (!useModel) {
    return (
      <div className="rounded-md border border-line bg-surface px-3.5 py-2.5 text-[12.5px] text-muted">
        <span className="font-medium text-fg">Built-in Operator.</span> Answers a fixed set of questions with no AI model; nothing leaves this machine.{" "}
        {configured ? null : (
          <>
            For open-ended questions, <Link to="/settings" className="text-accent hover:underline">connect a model</Link>.
          </>
        )}
      </div>
    );
  }
  return (
    <div className={cn("rounded-md border px-3.5 py-2.5 text-[12.5px]", local ? "border-line bg-surface text-muted" : "border-warning/40 bg-warning/10 text-fg")}>
      {local ? (
        <>
          <span className="font-medium text-fg">Local model.</span> Tool results go to {host}, on this machine.
        </>
      ) : (
        <>
          <span className="font-medium">Cloud model.</span> Tool results (process names, paths shown as ~, package names, counts) are sent to {host}. Instruction-file text, environment values
          and keys are never included.
        </>
      )}
    </div>
  );
}

function AnswerView({ answer, onReview }: { answer: OperatorAnswer; onReview: (p: Proposal) => void }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="flex flex-col gap-2.5 rounded-lg border border-line bg-surface p-3.5">
      <div className="flex items-center gap-2 text-[11px] text-faint">
        <Badge tone={answer.mode === "builtin" ? "neutral" : "info"}>{answer.mode === "builtin" ? "built-in" : answer.model}</Badge>
        {answer.data_sent_to ? <span>data sent to {answer.data_sent_to}</span> : <span>nothing sent off this machine</span>}
      </div>
      <div className="whitespace-pre-wrap break-words text-[13px] leading-relaxed text-fg">{answer.answer}</div>
      {answer.proposals.length > 0 ? (
        <div className="flex flex-col gap-2">
          {answer.proposals.map((p, i) => (
            <div key={i} className="flex items-center justify-between gap-3 rounded-md border border-line bg-raised px-3 py-2">
              <div className="min-w-0">
                <div className="truncate text-[12.5px] font-medium text-fg">{p.title}</div>
                <div className="truncate text-[11.5px] text-muted">{p.reason}</div>
              </div>
              <Button size="sm" variant={p.can_execute ? "primary" : "secondary"} disabled={!p.can_execute} onClick={() => onReview(p)}>
                Review
              </Button>
            </div>
          ))}
          <div className="text-[11px] text-faint">Proposals open a dry run. Nothing changes until you confirm it there.</div>
        </div>
      ) : null}
      {answer.steps.length > 0 ? (
        <div>
          <button onClick={() => setOpen((o) => !o)} className="flex items-center gap-1 text-[11.5px] text-muted hover:text-fg" aria-expanded={open}>
            <ChevronRight className={cn("size-3 transition-transform", open && "rotate-90")} aria-hidden />
            <Wrench className="size-3" aria-hidden /> {answer.steps.length} lookup{answer.steps.length === 1 ? "" : "s"}
          </button>
          {open ? (
            <ol className="mt-1.5 space-y-1 pl-5 text-[11.5px]">
              {answer.steps.map((st, i) => (
                <li key={i} className={st.ok ? "text-muted" : "text-danger"}>
                  <span className="text-fg">{TOOL_LABEL[st.tool] ?? st.tool}</span>
                  {Object.keys((st.arguments as Record<string, unknown>) ?? {}).length ? (
                    <code className="ml-1.5 text-faint">{JSON.stringify(st.arguments)}</code>
                  ) : null}{" "}
                  — {st.summary}
                </li>
              ))}
            </ol>
          ) : null}
        </div>
      ) : null}
    </div>
  );
}
