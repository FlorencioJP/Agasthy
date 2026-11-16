import type { GraphView } from "@agasthy/shared-types";
import { Graph, layout } from "@dagrejs/dagre";

export const NODE_WIDTH = 196;
export const NODE_HEIGHT = 44;

export interface Positioned {
  id: string;
  x: number;
  y: number;
}

/**
 * Left-to-right layered layout. Edges point from dependents to what they
 * depend on, so projects sit on the left and runtimes/packages on the right.
 */
export function layoutGraph(view: GraphView): Map<string, Positioned> {
  const g = new Graph({ multigraph: true });
  g.setGraph({ rankdir: "LR", nodesep: 14, ranksep: 110, marginx: 24, marginy: 24 });
  g.setDefaultEdgeLabel(() => ({}));
  for (const n of view.nodes) g.setNode(n.id, { width: NODE_WIDTH, height: NODE_HEIGHT });
  for (const e of view.edges) {
    if (g.hasNode(e.from_id) && g.hasNode(e.to_id)) g.setEdge(e.from_id, e.to_id, {}, e.id);
  }
  layout(g);
  const out = new Map<string, Positioned>();
  for (const id of g.nodes()) {
    const n = g.node(id) as { x: number; y: number } | undefined;
    if (n) out.set(id, { id, x: n.x - NODE_WIDTH / 2, y: n.y - NODE_HEIGHT / 2 });
  }
  return out;
}
