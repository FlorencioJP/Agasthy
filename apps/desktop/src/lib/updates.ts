import { isTauri } from "@tauri-apps/api/core";
import { useSyncExternalStore } from "react";

/**
 * Update checks against GitHub Releases. The updater verifies each update's
 * signature against the public key built into the app, and nothing is
 * installed without the user clicking "Install and restart".
 */
export type UpdateState =
  | { status: "idle" }
  | { status: "checking" }
  | { status: "none"; checkedAt: number }
  | { status: "available"; version: string; notes: string | null }
  | { status: "installing"; downloaded: number; total: number | null }
  | { status: "error"; message: string };

const AUTO_KEY = "agasthy.updates.auto";

let state: UpdateState = { status: "idle" };
const listeners = new Set<() => void>();
let pending: { downloadAndInstall: (cb: (e: unknown) => void) => Promise<void> } | null = null;
let dismissed = false;

function set(s: UpdateState) {
  state = s;
  for (const l of listeners) l();
}

export function useUpdateState(): UpdateState {
  return useSyncExternalStore(
    (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    () => state,
  );
}

export function autoCheckEnabled(): boolean {
  try {
    return localStorage.getItem(AUTO_KEY) !== "false";
  } catch {
    return true;
  }
}

export function setAutoCheck(on: boolean) {
  try {
    localStorage.setItem(AUTO_KEY, on ? "true" : "false");
  } catch {
    // Storage unavailable: the default (on) applies.
  }
}

/** Only in the real desktop app (never in tests or a plain browser). */
export function updatesSupported(): boolean {
  return typeof window !== "undefined" && isTauri() && !window.__AGASTHY_TEST_IPC__;
}

export async function checkForUpdate(): Promise<void> {
  if (!updatesSupported() || state.status === "checking" || state.status === "installing") return;
  set({ status: "checking" });
  try {
    const { check } = await import("@tauri-apps/plugin-updater");
    const u = await check();
    if (u) {
      pending = u as unknown as typeof pending;
      dismissed = false;
      set({ status: "available", version: u.version, notes: u.body ?? null });
    } else {
      set({ status: "none", checkedAt: Date.now() });
    }
  } catch (e) {
    set({ status: "error", message: e instanceof Error ? e.message : String(e) });
  }
}

export async function installUpdate(): Promise<void> {
  if (!pending) return;
  let downloaded = 0;
  let total: number | null = null;
  set({ status: "installing", downloaded, total });
  try {
    await pending.downloadAndInstall((ev: unknown) => {
      const e = ev as { event: string; data?: { contentLength?: number; chunkLength?: number } };
      if (e.event === "Started") total = e.data?.contentLength ?? null;
      if (e.event === "Progress") downloaded += e.data?.chunkLength ?? 0;
      set({ status: "installing", downloaded, total });
    });
    const { relaunch } = await import("@tauri-apps/plugin-process");
    await relaunch();
  } catch (e) {
    set({ status: "error", message: e instanceof Error ? e.message : String(e) });
  }
}

export function dismissUpdate() {
  dismissed = true;
  for (const l of listeners) l();
}

export function isDismissed() {
  return dismissed;
}
