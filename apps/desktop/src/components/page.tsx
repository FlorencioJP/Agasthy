import { cn } from "@agasthy/ui";
import type { ReactNode } from "react";

export function PageHeader({ title, subtitle, actions }: { title: string; subtitle?: ReactNode; actions?: ReactNode }) {
  return (
    <header
      data-tauri-drag-region
      className="sticky top-0 z-10 flex h-[52px] shrink-0 items-center justify-between gap-4 border-b border-line bg-bg/90 px-5 backdrop-blur"
    >
      <div data-tauri-drag-region className="flex min-w-0 items-baseline gap-3">
        <h1 className="text-[14px] font-semibold text-fg">{title}</h1>
        {subtitle ? <div className="truncate text-[12px] text-muted">{subtitle}</div> : null}
      </div>
      {actions ? <div className="flex items-center gap-2">{actions}</div> : null}
    </header>
  );
}

export function Panel({
  title,
  actions,
  children,
  className,
  bodyClassName,
}: {
  title?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  className?: string;
  bodyClassName?: string;
}) {
  return (
    <section className={cn("rounded-lg border border-line bg-surface", className)}>
      {title ? (
        <div className="flex h-9 items-center justify-between border-b border-line px-3.5">
          <h2 className="text-[12px] font-medium text-muted">{title}</h2>
          {actions}
        </div>
      ) : null}
      <div className={cn("p-3.5", bodyClassName)}>{children}</div>
    </section>
  );
}

export function Stat({ label, value, hint, onClick }: { label: string; value: ReactNode; hint?: ReactNode; onClick?: () => void }) {
  const Comp = onClick ? "button" : "div";
  return (
    <Comp
      onClick={onClick}
      className={cn(
        "flex w-full min-w-0 flex-col items-start gap-0.5 overflow-hidden rounded-md px-3 py-2.5 text-left",
        onClick && "transition-colors hover:bg-hover",
      )}
    >
      <span className="text-[11.5px] text-muted">{label}</span>
      <span className="tabular text-[19px] font-semibold leading-7 text-fg">{value}</span>
      {hint ? <span className="block w-full truncate text-[11.5px] text-faint">{hint}</span> : null}
    </Comp>
  );
}

export function Kv({ items }: { items: [string, ReactNode][] }) {
  return (
    <dl className="grid grid-cols-[minmax(110px,auto)_1fr] gap-x-4 gap-y-1.5 text-[12.5px]">
      {items.map(([k, v]) => (
        <div key={k} className="contents">
          <dt className="text-muted">{k}</dt>
          <dd className="min-w-0 break-words text-fg">{v}</dd>
        </div>
      ))}
    </dl>
  );
}
