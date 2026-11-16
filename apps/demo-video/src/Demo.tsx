import type { ReactNode } from "react";
import { AbsoluteFill, Sequence, interpolate, spring, useCurrentFrame, useVideoConfig } from "remotion";
import { ACCENT, Backdrop, Caption, Cursor, FULL, Highlight, Logo, SceneFade, Window } from "./kit";

const W = 1360; // window width on screen = screenshot width, so overlays use shot coordinates directly
const TOP = 205;

type Scene = { d: number; el: ReactNode };

function Intro() {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const s = spring({ frame, fps, config: { damping: 200 }, durationInFrames: 30 });
  const tagline = "One intelligence layer for your developer machine.";
  const shown = Math.floor(interpolate(frame, [22, 70], [0, tagline.length], { extrapolateLeft: "clamp", extrapolateRight: "clamp" }));
  const chips = ["Repositories", "Runtimes", "Packages", "Knowledge graph", "AI agents", "Tokens", "Safe cleanup"];
  return (
    <AbsoluteFill style={{ alignItems: "center", justifyContent: "center", flexDirection: "column", gap: 26 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 26, opacity: s, transform: `scale(${0.9 + 0.1 * s})` }}>
        <Logo size={120} />
        <div style={{ color: "#f5f3ef", fontSize: 104, fontWeight: 800, letterSpacing: 14 }}>AGASTHY</div>
      </div>
      <div style={{ color: "rgba(245,243,239,0.86)", fontSize: 40, fontWeight: 500, height: 52 }}>
        {tagline.slice(0, shown)}
        <span style={{ opacity: frame % 20 < 10 ? 1 : 0, color: ACCENT }}>|</span>
      </div>
      <div style={{ display: "flex", gap: 12, marginTop: 8 }}>
        {chips.map((c, i) => {
          const cs = spring({ frame: frame - 62 - i * 4, fps, config: { damping: 200 }, durationInFrames: 18 });
          return (
            <div
              key={c}
              style={{
                opacity: cs,
                transform: `translateY(${(1 - cs) * 14}px)`,
                color: "#f5f3ef",
                fontSize: 22,
                padding: "8px 16px",
                borderRadius: 999,
                border: "1px solid rgba(255,255,255,0.16)",
                background: "rgba(255,255,255,0.05)",
              }}
            >
              {c}
            </div>
          );
        })}
      </div>
    </AbsoluteFill>
  );
}

function Outro() {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const s = spring({ frame, fps, config: { damping: 200 }, durationInFrames: 30 });
  const line = (delay: number) => spring({ frame: frame - delay, fps, config: { damping: 200 }, durationInFrames: 20 });
  return (
    <AbsoluteFill style={{ alignItems: "center", justifyContent: "center", flexDirection: "column", gap: 20 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 22, opacity: s }}>
        <Logo size={96} />
        <div style={{ color: "#f5f3ef", fontSize: 84, fontWeight: 800, letterSpacing: 12 }}>AGASTHY</div>
      </div>
      <div style={{ color: "rgba(245,243,239,0.9)", fontSize: 38, opacity: line(12) }}>One intelligence layer for your developer machine.</div>
      <div style={{ display: "flex", gap: 14, marginTop: 10, opacity: line(24) }}>
        {["Free & open source", "Local-first", "No account", "No telemetry"].map((t) => (
          <div key={t} style={{ color: "#f5f3ef", fontSize: 24, padding: "8px 18px", borderRadius: 999, border: "1px solid rgba(255,255,255,0.18)" }}>
            {t}
          </div>
        ))}
      </div>
      <div style={{ marginTop: 26, opacity: line(38), textAlign: "center" }}>
        <div style={{ color: ACCENT, fontSize: 34, fontWeight: 700 }}>github.com/HMAKT99/Agasthy</div>
        <div style={{ color: "rgba(245,243,239,0.65)", fontSize: 24, marginTop: 8 }}>macOS · Windows preview · Linux from source</div>
      </div>
    </AbsoluteFill>
  );
}

const scenes: Scene[] = [
  { d: 100, el: <Intro /> },
  {
    d: 150,
    el: (
      <>
        <Caption kicker="Overview" title="Know what needs attention, right now" sub="Ranked insights from live and scanned data" />
        <Window shot="overview" top={TOP} width={W} to={{ cx: 760, cy: 300, zoom: 1.26 }} at={30} until={110}>
          <Highlight r={{ x: 228, y: 108, w: 1112, h: 66 }} at={70} label="3 processes left running" />
        </Window>
      </>
    ),
  },
  {
    d: 130,
    el: (
      <>
        <Caption kicker="Discovery" title="Every repository, app, tool and package" sub="Branches, uncommitted work, remotes, with evidence for every fact" />
        <Window shot="discovery" top={TOP} width={W} to={{ cx: 960, cy: 420, zoom: 1.18 }} at={30} until={110}>
          <Highlight r={{ x: 228, y: 172, w: 708, h: 24 }} at={34} />
          <Highlight r={{ x: 944, y: 345, w: 410, h: 120 }} at={70} label="Evidence" />
        </Window>
      </>
    ),
  },
  {
    d: 95,
    el: (
      <>
        <Caption kicker="Runtimes" title="Which Python actually runs?" sub="Active and shadowed copies, and how each was installed" />
        <Window shot="discovery-tools" top={TOP} width={W}>
          <Highlight r={{ x: 228, y: 490, w: 1128, h: 24 }} at={28} label="shadowed" />
          <Highlight r={{ x: 228, y: 651, w: 1128, h: 24 }} at={44} />
        </Window>
      </>
    ),
  },
  {
    d: 130,
    el: (
      <>
        <Caption kicker="Machine Graph" title="Your machine as a knowledge graph" sub="Projects, runtimes, dependencies and packages, connected" />
        <Window shot="graph" top={TOP} width={W} from={FULL} to={{ cx: 600, cy: 470, zoom: 1.45 }} at={20} until={120} />
      </>
    ),
  },
  {
    d: 140,
    el: (
      <>
        <Caption kicker="Impact Analysis" title="See what breaks before you remove anything" sub="“What if I remove openssl@3?”" />
        <Window shot="impact" top={TOP} width={W} to={{ cx: 680, cy: 620, zoom: 1.3 }} at={70} until={125}>
          <Highlight r={{ x: 1046, y: 171, w: 66, h: 24 }} at={28} label="high risk" />
          <Highlight r={{ x: 228, y: 640, w: 897, h: 205 }} at={80} label="5 projects affected" />
        </Window>
      </>
    ),
  },
  {
    d: 125,
    el: (
      <>
        <Caption kicker="AI Ecosystem" title="Every AI tool, MCP server and instruction file" sub="Duplicates, orphans and conflicting rules, flagged" />
        <Window shot="ai-instructions" top={TOP} width={W}>
          <Highlight r={{ x: 228, y: 370, w: 1128, h: 24 }} at={30} label="duplicate" />
          <Highlight r={{ x: 228, y: 458, w: 1128, h: 62 }} at={55} label="read by no installed tool" />
        </Window>
      </>
    ),
  },
  {
    d: 150,
    el: (
      <>
        <Caption kicker="Agents" title="Every coding agent, live" sub="Claude Code, Codex, Cursor, Gemini CLI, Hermes, and what they leave running" />
        <Window shot="agents" top={TOP} width={W} to={{ cx: 784, cy: 300, zoom: 1.22 }} at={60} until={130}>
          <Highlight r={{ x: 1127, y: 72, w: 212, h: 75 }} at={30} />
          <Highlight r={{ x: 228, y: 160, w: 1112, h: 222 }} at={70} label="started by agents that already exited" />
        </Window>
      </>
    ),
  },
  {
    d: 130,
    el: (
      <>
        <Caption kicker="Token Usage" title="What your agents actually cost" sub="Per day, project and model, from their own local logs" />
        <Window shot="usage" top={TOP} width={W} to={{ cx: 784, cy: 250, zoom: 1.3 }} at={30} until={110}>
          <Highlight r={{ x: 243, y: 121, w: 1082, h: 88 }} at={40} />
        </Window>
      </>
    ),
  },
  {
    d: 95,
    el: (
      <>
        <Caption kicker="Processes" title="Every process, its port and its project" />
        <Window shot="processes" top={TOP} width={W}>
          <Highlight r={{ x: 228, y: 222, w: 1128, h: 24 }} at={30} label=":8000 · ml-pipeline" />
        </Window>
      </>
    ),
  },
  {
    d: 95,
    el: (
      <>
        <Caption kicker="Storage & Cleanup" title="Free disk space without breaking anything" sub="Only rebuildable folders Git ignores, into a restorable quarantine" />
        <Window shot="storage" top={TOP} width={W} to={{ cx: 760, cy: 360, zoom: 1.15 }} at={20} until={90} />
      </>
    ),
  },
  {
    d: 125,
    el: (
      <>
        <Caption kicker="AI Operator" title="Ask your machine" sub="Answers from real data. Changes are only proposed." />
        <Window shot="operator" top={TOP} width={W}>
          <Highlight r={{ x: 975, y: 147, w: 168, h: 36 }} at={20} />
          <Cursor a={{ x: 760, y: 760 }} b={{ x: 1090, y: 536 }} at={50} until={88} />
        </Window>
      </>
    ),
  },
  {
    d: 130,
    el: (
      <>
        <Caption kicker="Safe by design" title="Nothing changes until you confirm" sub="Every action is a dry run first, then verified and logged" />
        <Window shot="review" top={TOP} width={W} to={{ cx: 680, cy: 430, zoom: 1.35 }} at={15} until={70}>
          <Highlight r={{ x: 361, y: 590, w: 380, h: 42 }} at={60} label="dry run" />
          <Cursor a={{ x: 600, y: 760 }} b={{ x: 930, y: 610 }} at={80} until={112} />
        </Window>
      </>
    ),
  },
  {
    d: 130,
    el: (
      <>
        <Caption kicker="Security & Snapshots" title="Check what's outdated. Rebuild your setup anywhere." />
        <Window shot="security" top={300} width={880} left={60} />
        <Window shot="snapshots" top={300} width={880} left={980} />
      </>
    ),
  },
  { d: 150, el: <Outro /> },
];

export const OVERLAP = 0;
export const DURATION = scenes.reduce((n, s) => n + s.d, 0) - OVERLAP * (scenes.length - 1);

export function Demo() {
  let start = 0;
  return (
    <Backdrop>
      {scenes.map((s, i) => {
        const from = start;
        start += s.d - OVERLAP;
        return (
          <Sequence key={i} from={from} durationInFrames={s.d}>
            <SceneFade duration={s.d} fade={i === 0 ? 1 : 9}>
              {s.el}
            </SceneFade>
          </Sequence>
        );
      })}
    </Backdrop>
  );
}
