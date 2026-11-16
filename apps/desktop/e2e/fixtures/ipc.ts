/**
 * DEVELOPMENT FIXTURE — automated tests only.
 *
 * A stand-in for the Tauri backend so UI tests can run in a browser. Every
 * value here is synthetic and labelled as such ("fixture-…", example.invalid);
 * it is never bundled into the application.
 */
import type { Page } from "@playwright/test";

export async function installFixtureIpc(page: Page): Promise<void> {
  await page.addInitScript(() => {
    const now = Date.now();
    const avail = <T,>(value: T) => ({ status: "available", value });
    const repoAttrs = {
      scan_root: "/fixture/code",
      git_dir_kind: "directory",
      head: avail({ branch: "main", detached: false, unborn: false, commit_sha: "0123456789abcdef0123456789abcdef01234567", last_commit_at: now - 3600_000 }),
      working_tree: avail({ clean: false, staged: 0, modified: 2, untracked: 1, conflicted: 0 }),
      local_branches: avail(3),
      remotes: [{ name: "origin", url: "https://example.invalid/fixture/widget.git", credentials_redacted: false, parsed: null }],
      primary_remote: null,
      manifests: ["package.json"],
      languages: [{ language: "TypeScript", files: 12 }],
      primary_language: "TypeScript",
      frameworks: ["React"],
      license: { spdx: "Apache-2.0", source: "LICENSE" },
      size_bytes: avail(1048576),
      clones_of_same_remote: 1,
    };
    const repo = {
      id: "repository:/fixture/code/fixture-widget",
      kind: "repository",
      name: "fixture-widget",
      path: "/fixture/code/fixture-widget",
      version: "0123456789ab",
      source: "git",
      attributes: repoAttrs,
      first_seen: now - 60_000,
      last_seen: now,
    };
    const unavailableGpu = { status: "unavailable", reason: "fixture: not measured" };
    const responses: Record<string, unknown> = {
      app_info: {
        version: "0.0.0-fixture",
        platform: { os: "fixture-os", os_version: null, arch: "fixture-arch", support_level: "skeleton" },
        data_dir: "/fixture/data",
        db_path: "/fixture/data/agasthy.db",
        schema_version: 1,
        search_dirs: [{ path: "/fixture/bin", origin: "process_path" }],
        command_allowlist: ["git"],
      },
      telemetry: {
        taken_at: now,
        host: { hostname: "fixture-host", os_name: "FixtureOS", os_version: "1.0", kernel_version: null, cpu_arch: "fixture-arch", uptime_secs: 3600 },
        cpu: { status: "unavailable", reason: "collecting the first CPU sample" },
        memory: avail({ total_bytes: 8 * 1024 ** 3, used_bytes: 2 * 1024 ** 3, available_bytes: 6 * 1024 ** 3, swap_total_bytes: 0, swap_used_bytes: 0 }),
        load_average: { status: "unsupported", platform: "fixture-os" },
        disks: avail([]),
        process_count: avail(1),
        top_by_memory: avail([{ pid: 1, name: "fixture-process", cpu_percent: unavailableGpu, memory_bytes: 1024, exe: null }]),
        top_by_cpu: { status: "unavailable", reason: "collecting the first CPU sample" },
        gpu_utilisation: unavailableGpu,
      },
      gpus: unavailableGpu,
      overview: {
        counts: [
          { kind: "repository", count: 1 },
          { kind: "application", count: 0 },
          { kind: "binary", count: 0 },
          { kind: "runtime", count: 0 },
          { kind: "package", count: 0 },
          { kind: "package_manager", count: 0 },
          { kind: "remote_project", count: 0 },
        ],
        recent: [repo],
        latest_scans: [
          { id: 1, kind: "repositories", status: "completed", started_at: now - 5000, finished_at: now - 4000, entities_found: 1, entities_removed: 0, issues: [] },
          { id: 2, kind: "ai", status: "completed", started_at: now - 3000, finished_at: now - 2000, entities_found: 3, entities_removed: 0, issues: [] },
        ],
        scan_roots: 1,
        dirty_repositories: 1,
        duplicate_clone_links: 0,
        binaries_installed_by_package: 0,
      },
      list_scan_roots: [{ id: 1, path: "/fixture/code", enabled: true, max_depth: 6, added_at: now }],
      running_scans: [],
      list_scans: [],
      list_audit: [],
      entity_detail: {
        entity: repo,
        evidence: [{ id: 1, kind: "git_directory", detail: "`.git` directory present", source: "/fixture/code/fixture-widget/.git", confidence: "verified", observed_at: now }],
        outgoing: [],
        incoming: [],
      },
    };
    const runtime = {
      id: "runtime:/fixture/bin/node",
      kind: "runtime",
      name: "Node.js",
      path: "/fixture/bin/node",
      version: "22.0.0",
      source: "path",
      attributes: { tool: "node", display_name: "Node.js", category: "runtime", active: true, rank: 0, version: avail("22.0.0") },
      first_seen: now,
      last_seen: now,
    };
    const graphView = {
      nodes: [
        { id: repo.id, kind: "repository", name: "fixture-widget", detail: "TypeScript" },
        { id: runtime.id, kind: "runtime", name: "Node.js", detail: "22.0.0" },
        { id: "dependency:npm:fixture-lib", kind: "dependency", name: "fixture-lib", detail: "npm" },
      ],
      edges: [
        { id: "e1", kind: "requires_runtime", from_id: repo.id, to_id: runtime.id, confidence: "verified", attributes: null },
        { id: "e2", kind: "depends_on", from_id: repo.id, to_id: "dependency:npm:fixture-lib", confidence: "verified", attributes: null },
      ],
      truncated: false,
      center: null,
    };
    responses.graph_overview = graphView;
    responses.graph_neighborhood = { ...graphView, center: repo.id };
    responses.shared_dependencies = [];
    responses.runtime_impact = {
      target: { id: runtime.id, path: "/fixture/bin/node", version: "22.0.0", active: true, rank: 0 },
      runtime: "node",
      other_copies: [],
      items: [
        {
          project: { id: repo.id, kind: "repository", name: "fixture-widget", path: repo.path },
          requirements: ["package.json: >=20"],
          outcome: "breaks",
          explanation: "No other node is on PATH; the project could not run its node tooling.",
          fallback: null,
        },
      ],
      breaks: 1,
      falls_back: 0,
      unaffected: 0,
      unknown: 0,
      limitations: ["fixture limitation"],
    };
    const aiTool = {
      id: "ai_tool:fixture-assistant",
      kind: "ai_tool",
      name: "Fixture Assistant",
      path: "/fixture/bin/assistant",
      version: "1.0.0",
      source: "ai",
      attributes: {
        tool: "fixture-assistant",
        vendor: "Fixture Vendor",
        category: "cli",
        installed: true,
        signals: [{ kind: "binary", path: "/fixture/bin/assistant", detail: null }],
        local_endpoint: null,
      },
      first_seen: now,
      last_seen: now,
    };
    const fileAttrs = (name: string, text: string) => ({
      file_type: "instructions",
      scope: "project",
      tools: ["fixture-assistant"],
      project: "/fixture/code/fixture-widget",
      relative_path: name,
      size_bytes: 40,
      modified_at: now,
      sha256: "0".repeat(64),
      lines: 2,
      headings: ["Fixture"],
      text,
      text_truncated: false,
      redactions: 0,
      orphaned: false,
      duplicates: [],
      overlaps: [],
      directives: [],
      references: [],
    });
    const fileA = {
      id: "ai_file:/fixture/code/fixture-widget/AGENTS.md",
      kind: "ai_config_file",
      name: "AGENTS.md",
      path: "/fixture/code/fixture-widget/AGENTS.md",
      version: null,
      source: "ai",
      attributes: fileAttrs("AGENTS.md", "# Fixture\nUse fixture-pm for installs.\n"),
      first_seen: now,
      last_seen: now,
    };
    const fileB = { ...fileA, id: "ai_file:/fixture/home/GLOBAL.md", name: "GLOBAL.md", path: "/fixture/home/GLOBAL.md", attributes: { ...fileAttrs("GLOBAL.md", "x"), scope: "global", project: null } };
    responses.list_edges = [
      { id: "c1", kind: "potential_conflict", from_id: fileA.id, to_id: fileB.id, confidence: "probable", attributes: { topic: "package_manager" } },
    ];
    const byKind: Record<string, unknown[]> = { repository: [repo], ai_tool: [aiTool], ai_config_file: [fileA, fileB] };
    window.__AGASTHY_TEST_IPC__ = async <T,>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
      if (cmd === "list_entities") {
        const q = (args?.query ?? {}) as { kind?: string | null };
        return (q.kind ? (byKind[q.kind] ?? []) : [repo]) as T;
      }
      if (cmd === "entity_detail" && args?.id === fileA.id) {
        return { entity: fileA, evidence: [], outgoing: [], incoming: [] } as T;
      }
      if (cmd === "entity_detail" && args?.id === runtime.id) {
        return { entity: runtime, evidence: [], outgoing: [], incoming: [] } as T;
      }
      if (cmd in responses) return responses[cmd] as T;
      throw `fixture: no response for ${cmd}`;
    };
  });
}
