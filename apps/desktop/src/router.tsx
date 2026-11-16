import { createHashHistory, createRootRoute, createRoute, createRouter } from "@tanstack/react-router";
import { Shell } from "./components/shell";
import { DiscoveryPage, type DiscoveryTab, TABS } from "./pages/discovery";
import { AI_TABS, AiPage, type AiSearch, type AiTab } from "./pages/ai";
import { GraphPage, type GraphMode, type GraphSearch } from "./pages/graph";
import { OverviewPage } from "./pages/overview";
import { PrivacyPage } from "./pages/privacy";
import { SettingsPage } from "./pages/settings";

const rootRoute = createRootRoute({ component: Shell });

const overviewRoute = createRoute({ getParentRoute: () => rootRoute, path: "/", component: OverviewPage });

export interface DiscoverySearch {
  tab: DiscoveryTab;
  q?: string;
  id?: string;
  filter?: string;
}

const discoveryRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/discovery",
  validateSearch: (s: Record<string, unknown>): DiscoverySearch => {
    const tab = TABS.some((t) => t.id === s.tab) ? (s.tab as DiscoveryTab) : "repositories";
    return {
      tab,
      q: typeof s.q === "string" && s.q ? s.q : undefined,
      id: typeof s.id === "string" && s.id ? s.id : undefined,
      filter: typeof s.filter === "string" && s.filter ? s.filter : undefined,
    };
  },
  component: DiscoveryPage,
});

const graphRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/graph",
  validateSearch: (s: Record<string, unknown>): GraphSearch => {
    const modes: GraphMode[] = ["projects", "software", "ai", "focus"];
    const focus = typeof s.focus === "string" && s.focus ? s.focus : undefined;
    let view = modes.includes(s.view as GraphMode) ? (s.view as GraphMode) : "projects";
    if (view === "focus" && !focus) view = "projects";
    const depth = Number(s.depth);
    return {
      view,
      focus,
      depth: depth >= 1 && depth <= 3 ? Math.floor(depth) : 1,
      sel: typeof s.sel === "string" && s.sel ? s.sel : undefined,
    };
  },
  component: GraphPage,
});

const aiRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/ai",
  validateSearch: (s: Record<string, unknown>): AiSearch => {
    const str = (v: unknown) => (typeof v === "string" && v ? v : undefined);
    const tab = AI_TABS.some((t) => t.id === s.tab) ? (s.tab as AiTab) : "overview";
    return { tab, id: str(s.id), q: str(s.q), filter: str(s.filter) };
  },
  component: AiPage,
});

const settingsRoute = createRoute({ getParentRoute: () => rootRoute, path: "/settings", component: SettingsPage });
const privacyRoute = createRoute({ getParentRoute: () => rootRoute, path: "/privacy", component: PrivacyPage });

const routeTree = rootRoute.addChildren([overviewRoute, discoveryRoute, graphRoute, aiRoute, settingsRoute, privacyRoute]);

// Hash history: the packaged app serves static files, so deep links must not hit the server.
export const router = createRouter({ routeTree, history: createHashHistory() });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
