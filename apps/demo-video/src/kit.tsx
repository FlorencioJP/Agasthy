import type { CSSProperties, ReactNode } from "react";
import { AbsoluteFill, Easing, Img, interpolate, spring, staticFile, useCurrentFrame, useVideoConfig } from "remotion";

/** Screenshot size in CSS pixels (captured at 2x). All regions use these coordinates. */
export const SHOT_W = 1360;
export const SHOT_H = 850;
/** On-screen window size. */
export const WIN_W = 1360;
export const WIN_H = (WIN_W * SHOT_H) / SHOT_W;
export const ACCENT = "#e8902f";
const FONT = "-apple-system, BlinkMacSystemFont, 'SF Pro Display', 'Inter', 'Helvetica Neue', sans-serif";

export type Rect = { x: number; y: number; w: number; h: number };
/** A camera view of the screenshot: centre point and zoom (1 = whole shot). */
export type View = { cx: number; cy: number; zoom: number };
export const FULL: View = { cx: SHOT_W / 2, cy: SHOT_H / 2, zoom: 1 };

const ease = Easing.bezier(0.45, 0, 0.2, 1);

export function useProgress(start: number, end: number) {
  const frame = useCurrentFrame();
  return interpolate(frame, [start, end], [0, 1], { extrapolateLeft: "clamp", extrapolateRight: "clamp", easing: ease });
}

export function Backdrop({ children }: { children?: ReactNode }) {
  return (
    <AbsoluteFill
      style={{
        background:
          "radial-gradient(1100px 700px at 15% 0%, rgba(232,144,47,0.28) 0%, transparent 60%), radial-gradient(900px 700px at 100% 100%, rgba(80,120,200,0.22) 0%, transparent 55%), #0d0e11",
        fontFamily: FONT,
      }}
    >
      {children}
    </AbsoluteFill>
  );
}

/** Fade a scene in and out so scenes cross-dissolve. */
export function SceneFade({ duration, children, fade = 10 }: { duration: number; children: ReactNode; fade?: number }) {
  const frame = useCurrentFrame();
  const opacity = interpolate(frame, [0, fade, duration - fade, duration], [0, 1, 1, 0], { extrapolateLeft: "clamp", extrapolateRight: "clamp" });
  return <AbsoluteFill style={{ opacity }}>{children}</AbsoluteFill>;
}

export function Caption({ kicker, title, sub }: { kicker: string; title: string; sub?: string }) {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const s = spring({ frame, fps, config: { damping: 200 }, durationInFrames: 22 });
  const y = interpolate(s, [0, 1], [24, 0]);
  return (
    <div style={{ position: "absolute", top: 46, left: 0, right: 0, textAlign: "center", opacity: s, transform: `translateY(${y}px)` }}>
      <div style={{ color: ACCENT, fontSize: 22, fontWeight: 600, letterSpacing: 3, textTransform: "uppercase" }}>{kicker}</div>
      <div style={{ color: "#f5f3ef", fontSize: 50, fontWeight: 700, letterSpacing: -1, marginTop: 6 }}>{title}</div>
      {sub ? <div style={{ color: "rgba(245,243,239,0.62)", fontSize: 24, marginTop: 6 }}>{sub}</div> : null}
    </div>
  );
}

/**
 * A macOS-style window showing one screenshot, with a camera that moves from
 * `from` to `to` between frames `at` and `until`, plus optional overlays drawn
 * in screenshot coordinates.
 */
export function Window({
  shot,
  from = FULL,
  to = FULL,
  at = 10,
  until = 90,
  top = 190,
  width = WIN_W,
  left,
  children,
  enter = true,
}: {
  shot: string;
  from?: View;
  to?: View;
  at?: number;
  until?: number;
  top?: number;
  width?: number;
  left?: number;
  children?: ReactNode;
  enter?: boolean;
}) {
  const frame = useCurrentFrame();
  const { fps, width: vw } = useVideoConfig();
  const p = useProgress(at, until);
  const v = { cx: from.cx + (to.cx - from.cx) * p, cy: from.cy + (to.cy - from.cy) * p, zoom: from.zoom + (to.zoom - from.zoom) * p };
  const height = (width * SHOT_H) / SHOT_W;
  const k = width / SHOT_W; // window px per shot px at zoom 1
  const scale = k * v.zoom;
  // Keep the view inside the shot.
  const halfW = SHOT_W / (2 * v.zoom);
  const halfH = SHOT_H / (2 * v.zoom);
  const cx = Math.min(Math.max(v.cx, halfW), SHOT_W - halfW);
  const cy = Math.min(Math.max(v.cy, halfH), SHOT_H - halfH);
  const tx = width / 2 - cx * scale;
  const ty = height / 2 - cy * scale;
  const rise = enter ? spring({ frame, fps, config: { damping: 200 }, durationInFrames: 26 }) : 1;
  const style: CSSProperties = {
    position: "absolute",
    top: top + (1 - rise) * 40,
    left: left ?? (vw - width) / 2,
    width,
    height,
    borderRadius: 14,
    overflow: "hidden",
    boxShadow: "0 0 0 1px rgba(255,255,255,0.08), 0 40px 120px rgba(0,0,0,0.55), 0 12px 32px rgba(0,0,0,0.35)",
    background: "#faf9f7",
    opacity: rise,
  };
  return (
    <div style={style}>
      <div style={{ position: "absolute", left: 0, top: 0, width: SHOT_W, height: SHOT_H, transformOrigin: "0 0", transform: `translate(${tx}px, ${ty}px) scale(${scale})` }}>
        <Img src={staticFile(`shots/${shot}.png`)} style={{ width: SHOT_W, height: SHOT_H, display: "block" }} />
        {children}
      </div>
      <div style={{ position: "absolute", top: 15, left: 16, display: "flex", gap: 8 }}>
        {["#ff5f57", "#febc2e", "#28c840"].map((c) => (
          <span key={c} style={{ width: 12, height: 12, borderRadius: 6, background: c }} />
        ))}
      </div>
    </div>
  );
}

/** A glowing ring around a region (screenshot coordinates), drawn on and pulsing. */
export function Highlight({ r, at, label }: { r: Rect; at: number; label?: string }) {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const s = spring({ frame: frame - at, fps, config: { damping: 18, stiffness: 120 } });
  const pulse = 0.55 + 0.45 * Math.sin(Math.max(0, frame - at) / 6);
  if (frame < at) return null;
  const pad = 6;
  return (
    <>
      <div
        style={{
          position: "absolute",
          left: r.x - pad,
          top: r.y - pad,
          width: r.w + pad * 2,
          height: r.h + pad * 2,
          borderRadius: 10,
          border: `2.5px solid ${ACCENT}`,
          boxShadow: `0 0 ${18 * pulse}px ${ACCENT}, inset 0 0 ${10 * pulse}px rgba(232,144,47,0.25)`,
          opacity: s,
          transform: `scale(${0.96 + 0.04 * s})`,
        }}
      />
      {label ? (
        <div
          style={{
            position: "absolute",
            left: r.x - pad,
            top: r.y - pad - 30,
            background: ACCENT,
            color: "#1b1206",
            fontSize: 14,
            fontWeight: 700,
            padding: "4px 10px",
            borderRadius: 7,
            opacity: s,
            fontFamily: FONT,
            whiteSpace: "nowrap",
          }}
        >
          {label}
        </div>
      ) : null}
    </>
  );
}

/** A mouse pointer gliding from a to b, clicking at the end. Screenshot coordinates. */
export function Cursor({ a, b, at, until }: { a: { x: number; y: number }; b: { x: number; y: number }; at: number; until: number }) {
  const frame = useCurrentFrame();
  const p = useProgress(at, until);
  if (frame < at - 8) return null;
  const x = a.x + (b.x - a.x) * p;
  const y = a.y + (b.y - a.y) * p;
  const click = frame - until;
  const ring = click >= 0 && click < 18 ? click / 18 : -1;
  return (
    <>
      {ring >= 0 ? (
        <div
          style={{
            position: "absolute",
            left: b.x - 22 * ring,
            top: b.y - 22 * ring,
            width: 44 * ring,
            height: 44 * ring,
            borderRadius: "50%",
            border: `2px solid ${ACCENT}`,
            opacity: 1 - ring,
          }}
        />
      ) : null}
      <svg width="22" height="30" viewBox="0 0 22 30" style={{ position: "absolute", left: x, top: y, filter: "drop-shadow(0 2px 3px rgba(0,0,0,0.35))", transform: click >= 0 && click < 5 ? "scale(0.88)" : "none" }}>
        <path d="M1 1 L1 23 L7 17.5 L11 28 L15 26.3 L11 16 L19 16 Z" fill="#111" stroke="#fff" strokeWidth="1.6" strokeLinejoin="round" />
      </svg>
    </>
  );
}

export function Logo({ size }: { size: number }) {
  return <Img src={staticFile("logo.svg")} style={{ width: size, height: size }} />;
}
