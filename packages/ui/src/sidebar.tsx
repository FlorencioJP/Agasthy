import type { ReactNode } from "react";
import { cn } from "./cn";

export function Sidebar({ header, children, footer }: { header?: ReactNode; children: ReactNode; footer?: ReactNode }) {
  return (
    <aside className="flex h-full w-[208px] shrink-0 flex-col border-r border-line bg-sidebar">
      {header}
      <nav className="flex-1 overflow-y-auto px-2 py-2" aria-label="Main">
        {children}
      </nav>
      {footer ? <div className="border-t border-line px-3 py-2.5">{footer}</div> : null}
    </aside>
  );
}

export function SidebarSection({ label, children }: { label?: string; children: ReactNode }) {
  return (
    <div className="mb-3">
      {label ? (
        <div className="px-2 pb-1 pt-1 text-[10.5px] font-medium uppercase tracking-[0.08em] text-faint">{label}</div>
      ) : null}
      <ul className="space-y-px">{children}</ul>
    </div>
  );
}

/** Class names for a sidebar link, so routers can render their own anchor. */
export function sidebarItemClass(active: boolean): string {
  return cn(
    "group flex h-7 items-center gap-2 rounded-md px-2 text-[13px] transition-colors [&_svg]:size-[15px] [&_svg]:shrink-0",
    active ? "bg-hover text-fg" : "text-muted hover:bg-hover/60 hover:text-fg",
  );
}
