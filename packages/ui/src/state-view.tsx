import { AlertTriangle, Ban, Inbox, Loader2, Lock } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "./cn";

export type StateKind = "loading" | "empty" | "error" | "permission-denied" | "unsupported";

const ICONS = {
  loading: Loader2,
  empty: Inbox,
  error: AlertTriangle,
  "permission-denied": Lock,
  unsupported: Ban,
} as const;

const TONES: Record<StateKind, string> = {
  loading: "text-muted",
  empty: "text-muted",
  error: "text-danger",
  "permission-denied": "text-warning",
  unsupported: "text-muted",
};

export interface StateViewProps {
  kind: StateKind;
  title: string;
  description?: ReactNode;
  action?: ReactNode;
  compact?: boolean;
  className?: string;
}

/**
 * The one way AGASTHY shows a non-data state. Loading, empty, error,
 * permission-denied and unsupported are visually distinct so that "nothing
 * found" is never confused with "could not look".
 */
export function StateView({ kind, title, description, action, compact, className }: StateViewProps) {
  const Icon = ICONS[kind];
  return (
    <div
      role={kind === "error" ? "alert" : "status"}
      aria-live={kind === "loading" ? "polite" : undefined}
      data-state={kind}
      className={cn(
        "flex flex-col items-center justify-center text-center",
        compact ? "gap-1.5 py-6" : "gap-2.5 py-14",
        className,
      )}
    >
      <Icon className={cn(compact ? "size-4" : "size-5", TONES[kind], kind === "loading" && "animate-spin")} aria-hidden />
      <div className="text-[13px] font-medium text-fg">{title}</div>
      {description ? <div className="max-w-md text-[12px] leading-5 text-muted">{description}</div> : null}
      {action ? <div className="mt-1.5">{action}</div> : null}
    </div>
  );
}
