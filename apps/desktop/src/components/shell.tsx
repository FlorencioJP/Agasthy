import { Sidebar, SidebarSection, StateView, Wordmark, sidebarItemClass } from "@agasthy/ui";
import { Link, Outlet, useRouterState } from "@tanstack/react-router";
import { Activity, Bot, Boxes, Camera, Gauge, GitCompareArrows, HardDrive, Loader2, Network, Settings, Shield, ShieldAlert, Sparkles } from "lucide-react";
import { inDesktopShell } from "../lib/ipc";
import { useAppInfo } from "../lib/queries";
import { SCAN_LABELS, useScanActivity } from "./scan-activity";

// Only sections with a working implementation appear here.
const NAV = [
  { to: "/", label: "Overview", icon: Gauge, exact: true },
  { to: "/operator", label: "AI Operator", icon: Sparkles, exact: false },
  { to: "/discovery", label: "Discovery", icon: Boxes, exact: false },
  { to: "/graph", label: "Machine Graph", icon: Network, exact: false },
  { to: "/ai", label: "AI Ecosystem", icon: Bot, exact: false },
  { to: "/processes", label: "Processes", icon: Activity, exact: false },
  { to: "/storage", label: "Storage & Cleanup", icon: HardDrive, exact: false },
  { to: "/security", label: "Security", icon: ShieldAlert, exact: false },
  { to: "/impact", label: "Impact Analysis", icon: GitCompareArrows, exact: false },
  { to: "/snapshots", label: "Snapshots", icon: Camera, exact: false },
] as const;

const SYSTEM_NAV = [
  { to: "/settings", label: "Settings", icon: Settings, exact: true },
  { to: "/privacy", label: "Privacy", icon: Shield, exact: true },
] as const;

function NavLink({ to, label, icon: Icon, exact }: { to: string; label: string; icon: typeof Gauge; exact: boolean }) {
  const path = useRouterState({ select: (s) => s.location.pathname });
  const active = exact ? path === to : path.startsWith(to);
  return (
    <li>
      <Link to={to} className={sidebarItemClass(active)} aria-current={active ? "page" : undefined}>
        <Icon aria-hidden />
        <span>{label}</span>
      </Link>
    </li>
  );
}

function ScanIndicator() {
  const { running } = useScanActivity();
  if (running.size === 0) return null;
  return (
    <div className="flex items-center gap-2 text-[11.5px] text-muted" role="status">
      <Loader2 className="size-3.5 animate-spin text-accent" aria-hidden />
      <span className="truncate">Scanning {[...running].map((k) => SCAN_LABELS[k]).join(", ")}</span>
    </div>
  );
}

export function Shell() {
  const info = useAppInfo();
  if (!inDesktopShell()) {
    return (
      <div className="flex h-full items-center justify-center">
        <StateView
          kind="unsupported"
          title="Open AGASTHY from the desktop app"
          description="This interface reads your machine through the AGASTHY desktop shell. In a plain browser there is no machine data to show, and AGASTHY does not display sample data instead. Run `pnpm dev` from the repository root."
        />
      </div>
    );
  }
  return (
    <div className="flex h-full">
      <Sidebar
        header={
          <div data-tauri-drag-region className="flex h-[52px] items-end px-4 pb-2.5 pl-[78px]">
            <Wordmark />
          </div>
        }
        footer={
          <div className="space-y-1.5">
            <ScanIndicator />
            <div className="flex items-center justify-between text-[11px] text-faint">
              <span>{info.data ? `v${info.data.version}` : ""}</span>
              <span>{info.data ? `${info.data.platform.os} · ${info.data.platform.arch}` : ""}</span>
            </div>
          </div>
        }
      >
        <SidebarSection>
          {NAV.map((n) => (
            <NavLink key={n.to} {...n} />
          ))}
        </SidebarSection>
        <SidebarSection label="System">
          {SYSTEM_NAV.map((n) => (
            <NavLink key={n.to} {...n} />
          ))}
        </SidebarSection>
      </Sidebar>
      <main className="flex min-w-0 flex-1 flex-col overflow-y-auto">
        <Outlet />
      </main>
    </div>
  );
}
