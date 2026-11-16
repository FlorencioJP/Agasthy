import type {
  AppInfo,
  EntityKind,
  AuditEntry,
  Availability,
  Entity,
  EntityDetail,
  EntityQuery,
  GpuInfo,
  Overview,
  GraphView,
  ImpactReport,
  OverviewKind,
  ScanEvent,
  SharedDependency,
  ScanKind,
  ScanRecord,
  ScanRoot,
  TelemetrySnapshot,
} from "@agasthy/shared-types";
import { invoke as tauriInvoke, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Invoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>;

declare global {
  interface Window {
    /**
     * Test-only IPC replacement installed by Playwright fixtures
     * (apps/desktop/e2e/fixtures). Never set in the shipped application.
     */
    __AGASTHY_TEST_IPC__?: Invoke;
  }
}

/** True when running inside the AGASTHY desktop shell (or a test fixture). */
export function inDesktopShell(): boolean {
  return typeof window !== "undefined" && (isTauri() || Boolean(window.__AGASTHY_TEST_IPC__));
}

const invoke: Invoke = (cmd, args) => {
  if (window.__AGASTHY_TEST_IPC__) return window.__AGASTHY_TEST_IPC__(cmd, args);
  return tauriInvoke(cmd, args);
};

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  telemetry: () => invoke<TelemetrySnapshot>("telemetry"),
  gpus: () => invoke<Availability<GpuInfo[]>>("gpus"),
  overview: () => invoke<Overview>("overview"),
  listScanRoots: () => invoke<ScanRoot[]>("list_scan_roots"),
  addScanRoot: (path: string) => invoke<ScanRoot>("add_scan_root", { path }),
  removeScanRoot: (id: number) => invoke<void>("remove_scan_root", { id }),
  setScanRootEnabled: (id: number, enabled: boolean) =>
    invoke<void>("set_scan_root_enabled", { id, enabled }),
  runScan: (kind: ScanKind) => invoke<ScanRecord>("run_scan", { kind }),
  runningScans: () => invoke<ScanKind[]>("running_scans"),
  listEntities: (query: EntityQuery) => invoke<Entity[]>("list_entities", { query }),
  entityDetail: (id: string) => invoke<EntityDetail | null>("entity_detail", { id }),
  listScans: (limit?: number) => invoke<ScanRecord[]>("list_scans", { limit }),
  listAudit: (limit?: number) => invoke<AuditEntry[]>("list_audit", { limit }),
  graphNeighborhood: (center: string, depth: number, kinds: EntityKind[]) =>
    invoke<GraphView>("graph_neighborhood", { center, depth, kinds }),
  graphOverview: (kind: OverviewKind) => invoke<GraphView>("graph_overview", { kind }),
  runtimeImpact: (id: string) => invoke<ImpactReport | null>("runtime_impact", { id }),
  sharedDependencies: (limit?: number) => invoke<SharedDependency[]>("shared_dependencies", { limit }),
};

export async function onScanEvent(handler: (e: ScanEvent) => void): Promise<() => void> {
  if (!isTauri()) return () => {};
  return listen<ScanEvent>("scan-event", (e) => handler(e.payload));
}

export async function pickFolder(): Promise<string | null> {
  const { open } = await import("@tauri-apps/plugin-dialog");
  const picked = await open({ directory: true, multiple: false, title: "Choose a folder to scan" });
  return typeof picked === "string" ? picked : null;
}

export async function revealInFileManager(path: string): Promise<void> {
  const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
  await revealItemInDir(path);
}

/** Normalise errors returned by Tauri commands (strings) and JS errors. */
export function errorMessage(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return "Unexpected error";
}
