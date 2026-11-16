import type { EntityKind, OverviewKind, ScanKind } from "@agasthy/shared-types";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { api } from "./ipc";

export const keys = {
  appInfo: ["app-info"] as const,
  telemetry: ["telemetry"] as const,
  gpus: ["gpus"] as const,
  overview: ["overview"] as const,
  roots: ["scan-roots"] as const,
  scans: ["scans"] as const,
  running: ["running-scans"] as const,
  audit: ["audit"] as const,
  entities: (kind: EntityKind | null, text: string) => ["entities", kind, text] as const,
  entity: (id: string) => ["entity", id] as const,
  graph: ["graph"] as const,
  impact: (id: string) => ["impact", id] as const,
  shared: ["shared-dependencies"] as const,
};

export const useAppInfo = () => useQuery({ queryKey: keys.appInfo, queryFn: api.appInfo, staleTime: Infinity });

export const useTelemetry = () =>
  useQuery({
    queryKey: keys.telemetry,
    queryFn: api.telemetry,
    refetchInterval: 2000,
    refetchIntervalInBackground: false,
  });

export const useGpus = () => useQuery({ queryKey: keys.gpus, queryFn: api.gpus, staleTime: Infinity });

export const useOverview = () => useQuery({ queryKey: keys.overview, queryFn: api.overview });

export const useScanRoots = () => useQuery({ queryKey: keys.roots, queryFn: api.listScanRoots });

export const useScans = (limit = 30) =>
  useQuery({ queryKey: [...keys.scans, limit], queryFn: () => api.listScans(limit) });

export const useAudit = (limit = 100) =>
  useQuery({ queryKey: [...keys.audit, limit], queryFn: () => api.listAudit(limit) });

export const useEntities = (kind: EntityKind | null, text: string, limit = 2000) =>
  useQuery({
    queryKey: keys.entities(kind, text),
    queryFn: () => api.listEntities({ kind, text: text.trim() || null, limit, offset: null }),
    placeholderData: (prev) => prev,
  });

export const useEntityDetail = (id: string | null) =>
  useQuery({
    queryKey: keys.entity(id ?? ""),
    queryFn: () => api.entityDetail(id as string),
    enabled: Boolean(id),
  });

export const useGraphOverview = (kind: OverviewKind, enabled: boolean) =>
  useQuery({ queryKey: [...keys.graph, "overview", kind], queryFn: () => api.graphOverview(kind), enabled });

export const useGraphNeighborhood = (center: string | null, depth: number, kinds: EntityKind[]) =>
  useQuery({
    queryKey: [...keys.graph, "focus", center, depth, kinds.join(",")],
    queryFn: () => api.graphNeighborhood(center as string, depth, kinds),
    enabled: Boolean(center),
  });

export const useRuntimeImpact = (id: string | null) =>
  useQuery({ queryKey: keys.impact(id ?? ""), queryFn: () => api.runtimeImpact(id as string), enabled: Boolean(id) });

export const useSharedDependencies = () => useQuery({ queryKey: keys.shared, queryFn: () => api.sharedDependencies(15) });

/** Invalidate everything derived from the database after a write. */
export function useInvalidateData() {
  const qc = useQueryClient();
  return () =>
    Promise.all(
      [keys.overview, keys.roots, keys.scans, keys.audit, ["entities"], ["entity"], keys.running, keys.graph, ["impact"], keys.shared].map((k) =>
        qc.invalidateQueries({ queryKey: k }),
      ),
    );
}

export function useRunScan() {
  const invalidate = useInvalidateData();
  return useMutation({
    mutationFn: (kind: ScanKind) => api.runScan(kind),
    onSettled: () => invalidate(),
  });
}
