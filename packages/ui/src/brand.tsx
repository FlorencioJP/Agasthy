import { cn } from "./cn";

/** The AGASTHY mark: an "A" drawn as a three-node graph. */
export function LogoMark({ className, title = "AGASTHY" }: { className?: string; title?: string }) {
  return (
    <svg viewBox="0 0 64 64" className={cn("size-5", className)} role="img" aria-label={title}>
      <g fill="none" strokeLinecap="round">
        <path d="M32 11 L18.5 51" stroke="currentColor" strokeWidth="5" />
        <path d="M32 11 L45.5 51" stroke="currentColor" strokeWidth="5" />
        <path d="M24 39 L40 39" stroke="var(--color-accent)" strokeWidth="3.2" strokeDasharray="0.2 5.4" />
      </g>
      <circle cx="32" cy="11" r="5.2" fill="var(--color-accent)" />
      <circle cx="18.5" cy="51" r="4" fill="var(--color-bg)" stroke="currentColor" strokeWidth="2.6" />
      <circle cx="45.5" cy="51" r="4" fill="var(--color-bg)" stroke="currentColor" strokeWidth="2.6" />
    </svg>
  );
}

export function Wordmark({ className }: { className?: string }) {
  return (
    <span className={cn("inline-flex items-center gap-2 text-fg", className)}>
      <LogoMark />
      <span className="font-mono text-[12.5px] font-semibold tracking-[0.22em]">AGASTHY</span>
    </span>
  );
}
