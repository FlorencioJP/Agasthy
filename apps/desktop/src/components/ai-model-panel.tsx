import type { ProviderConfig } from "@agasthy/shared-types";
import { Badge, Button, StateView } from "@agasthy/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { KeyRound } from "lucide-react";
import { useState } from "react";
import { api, errorMessage } from "../lib/ipc";
import { Panel } from "./page";

const field = "h-8 w-full rounded-md border border-line bg-surface px-2.5 text-[12.5px] text-fg focus:outline-none focus:ring-2 focus:ring-accent/50";

/** Settings → AI model. Off by default; the built-in Operator needs no model. */
export function AiModelPanel() {
  const qc = useQueryClient();
  const settings = useQuery({ queryKey: ["operator-settings"], queryFn: api.operatorSettings });
  const [draft, setDraft] = useState<ProviderConfig | null>(null);
  const [key, setKey] = useState("");
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const s = settings.data;
  const refresh = () => qc.invalidateQueries({ queryKey: ["operator-settings"] });

  if (settings.isLoading || !s) {
    return (
      <Panel title="AI model">
        <StateView kind={settings.isError ? "error" : "loading"} title={settings.isError ? errorMessage(settings.error) : "Loading"} compact />
      </Panel>
    );
  }

  const current = draft ?? s.provider;
  const preset = s.presets.find((p) => p.id === current?.preset);
  const isLocal = (url: string) => /^https?:\/\/(127\.0\.0\.1|localhost|\[::1\])(:|\/|$)/.test(url);
  const choose = (id: string) => {
    if (!id) return setDraft(null);
    const p = s.presets.find((x) => x.id === id);
    if (p) setDraft({ preset: p.id, kind: p.kind, base_url: p.base_url, model: p.default_model });
  };
  const run = async (f: () => Promise<void>, ok: string) => {
    setMessage(null);
    try {
      await f();
      setMessage({ ok: true, text: ok });
      await refresh();
    } catch (e) {
      setMessage({ ok: false, text: errorMessage(e) });
    }
  };

  return (
    <Panel title="AI model" actions={<span className="text-[11px] text-faint">optional · off by default</span>}>
      <div className="flex flex-col gap-3.5 text-[12.5px]">
        <p className="text-muted">
          The built-in Operator answers common questions with no model. A model adds open-ended questions. It can only call AGASTHY&rsquo;s read-only tools and
          propose actions you review; it cannot run commands or change anything.
        </p>
        <div className="grid grid-cols-[110px_1fr] items-center gap-x-3 gap-y-2.5">
          <label htmlFor="ai-provider" className="text-muted">
            Provider
          </label>
          <select id="ai-provider" className={field} value={current?.preset ?? ""} onChange={(e) => choose(e.target.value)}>
            <option value="">None (built-in only)</option>
            {s.presets.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
          {current ? (
            <>
              <label htmlFor="ai-url" className="text-muted">
                Endpoint
              </label>
              <input
                id="ai-url"
                className={field}
                value={current.base_url}
                placeholder="https://…"
                onChange={(e) => setDraft({ ...current, base_url: e.target.value.trim() })}
              />
              <label htmlFor="ai-model" className="text-muted">
                Model
              </label>
              <input id="ai-model" className={field} value={current.model} placeholder="model name" onChange={(e) => setDraft({ ...current, model: e.target.value.trim() })} />
              <span className="text-muted">Runs</span>
              <span>{isLocal(current.base_url) ? <Badge tone="success">on this machine</Badge> : <Badge tone="warning">in the cloud</Badge>}</span>
            </>
          ) : null}
        </div>
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="primary"
            disabled={draft === null}
            onClick={() => void run(async () => { await api.operatorSetProvider(draft); setDraft(null); }, draft ? "Saved." : "Model turned off.")}
          >
            Save
          </Button>
          {s.provider ? (
            <Button size="sm" variant="ghost" onClick={() => void run(async () => { await api.operatorSetProvider(null); setDraft(null); }, "Model turned off.")}>
              Turn off
            </Button>
          ) : null}
        </div>

        {current && (preset?.needs_key || current.kind === "anthropic" || !isLocal(current.base_url)) ? (
          <div className="flex flex-col gap-2 rounded-md border border-line p-3">
            <div className="flex items-center gap-2">
              <KeyRound className="size-3.5 text-muted" aria-hidden />
              <span className="font-medium text-fg">API key</span>
              {s.provider?.preset === current.preset ? (
                s.has_key ? <Badge tone="success">saved in Keychain</Badge> : <Badge>not set</Badge>
              ) : null}
            </div>
            <div className="text-[11.5px] text-muted">Stored in the operating system&rsquo;s credential store. Never written to AGASTHY&rsquo;s database, logs or files.</div>
            <div className="flex items-center gap-2">
              <input type="password" autoComplete="off" aria-label="API key" className={field} value={key} onChange={(e) => setKey(e.target.value)} placeholder="Paste a key" />
              <Button size="sm" disabled={!key.trim()} onClick={() => void run(async () => { await api.operatorSetKey(current.preset, key); setKey(""); }, "Key saved.")}>
                Save key
              </Button>
              {s.provider?.preset === current.preset && s.has_key ? (
                <Button size="sm" variant="ghost" onClick={() => void run(() => api.operatorDeleteKey(current.preset), "Key removed.")}>
                  Remove
                </Button>
              ) : null}
            </div>
          </div>
        ) : null}

        <label className="flex items-start gap-2.5 rounded-md border border-line p-3">
          <input
            type="checkbox"
            className="mt-0.5"
            checked={s.cloud_consent}
            onChange={(e) => void run(() => api.operatorSetCloudConsent(e.target.checked), e.target.checked ? "Cloud models allowed." : "Cloud models blocked.")}
          />
          <span>
            <span className="font-medium text-fg">Allow sending machine data to a cloud model</span>
            <span className="block text-[11.5px] text-muted">
              Needed for any non-local endpoint. Sent: the question and tool results (process names, paths with your home folder shown as ~, package names,
              sizes). Never sent: instruction-file text, environment values, keys. Each question is recorded in the audit log with where its data went.
            </span>
          </span>
        </label>
        {message ? (
          <div role="status" className={message.ok ? "text-success" : "text-danger"}>
            {message.text}
          </div>
        ) : null}
      </div>
    </Panel>
  );
}
