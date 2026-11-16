import type { ScanKind, ScanProgress, ScanRecord } from "@agasthy/shared-types";
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api, errorMessage, onScanEvent } from "../lib/ipc";
import { useInvalidateData } from "../lib/queries";

interface Activity {
  running: Set<ScanKind>;
  progress: Partial<Record<ScanKind, ScanProgress>>;
  lastError: Partial<Record<ScanKind, string>>;
  start: (kind: ScanKind) => Promise<ScanRecord | null>;
  /** Packages first, so the software scan can link binaries to packages. */
  startInventory: () => Promise<void>;
}

const Ctx = createContext<Activity | null>(null);

export function ScanActivityProvider({ children }: { children: ReactNode }) {
  const [running, setRunning] = useState<Set<ScanKind>>(new Set());
  const [progress, setProgress] = useState<Partial<Record<ScanKind, ScanProgress>>>({});
  const [lastError, setLastError] = useState<Partial<Record<ScanKind, string>>>({});
  const invalidate = useInvalidateData();
  const invalidateRef = useRef(invalidate);
  invalidateRef.current = invalidate;

  useEffect(() => {
    // Scans started before a reload are still running in the backend.
    api
      .runningScans()
      .then((kinds) => setRunning(new Set(kinds)))
      .catch(() => {});
    let unlisten: (() => void) | undefined;
    onScanEvent((e) => {
      if (e.stage === "progress" && e.progress) {
        setProgress((p) => ({ ...p, [e.kind]: e.progress ?? undefined }));
      } else {
        setProgress((p) => ({ ...p, [e.kind]: undefined }));
        void invalidateRef.current();
      }
    }).then((u) => (unlisten = u));
    return () => unlisten?.();
  }, []);

  const start = useCallback(async (kind: ScanKind) => {
    setRunning((r) => new Set(r).add(kind));
    setLastError((e) => ({ ...e, [kind]: undefined }));
    try {
      return await api.runScan(kind);
    } catch (e) {
      setLastError((x) => ({ ...x, [kind]: errorMessage(e) }));
      return null;
    } finally {
      setRunning((r) => {
        const n = new Set(r);
        n.delete(kind);
        return n;
      });
      setProgress((p) => ({ ...p, [kind]: undefined }));
      await invalidateRef.current();
    }
  }, []);

  const startInventory = useCallback(async () => {
    await start("packages");
    await start("software");
  }, [start]);

  const value = useMemo(
    () => ({ running, progress, lastError, start, startInventory }),
    [running, progress, lastError, start, startInventory],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useScanActivity(): Activity {
  const v = useContext(Ctx);
  if (!v) throw new Error("useScanActivity must be used inside ScanActivityProvider");
  return v;
}

export const SCAN_LABELS: Record<ScanKind, string> = {
  repositories: "Repositories",
  software: "Applications & tools",
  packages: "Packages",
};

export function progressLabel(p: ScanProgress | undefined): string {
  if (!p) return "Starting…";
  const phase: Record<string, string> = {
    searching: "Searching folders",
    inspecting: "Inspecting repositories",
    applications: "Reading applications",
    tools: "Probing tools",
    packages: "Reading",
    done: "Saving",
  };
  const base = phase[p.phase] ?? p.phase;
  const count = p.total != null ? ` ${p.done}/${p.total}` : p.done ? ` ${p.done}` : "";
  const cur = p.phase === "packages" && p.current ? ` ${p.current}` : "";
  return `${base}${cur}${count}`;
}
