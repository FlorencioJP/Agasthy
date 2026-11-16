import { cva, type VariantProps } from "class-variance-authority";
import type { HTMLAttributes } from "react";
import { cn } from "./cn";

export const badgeVariants = cva(
  "inline-flex items-center gap-1 rounded px-1.5 py-px text-[11px] font-medium leading-4 whitespace-nowrap [&_svg]:size-3",
  {
    variants: {
      tone: {
        neutral: "bg-raised text-muted border border-line",
        success: "bg-success/12 text-success",
        warning: "bg-warning/12 text-warning",
        danger: "bg-danger/12 text-danger",
        info: "bg-info/12 text-info",
        accent: "bg-accent/14 text-accent",
      },
    },
    defaultVariants: { tone: "neutral" },
  },
);

export type BadgeProps = HTMLAttributes<HTMLSpanElement> & VariantProps<typeof badgeVariants>;

export function Badge({ className, tone, ...props }: BadgeProps) {
  return <span className={cn(badgeVariants({ tone }), className)} {...props} />;
}

export type ConfidenceLevel = "verified" | "strong" | "probable" | "unknown";

const CONFIDENCE: Record<ConfidenceLevel, { tone: BadgeProps["tone"]; label: string; hint: string }> = {
  verified: { tone: "success", label: "Verified", hint: "Stated directly by the system or the tool's own metadata" },
  strong: { tone: "info", label: "Strong", hint: "Supported by package-manager metadata or file ownership" },
  probable: { tone: "warning", label: "Probable", hint: "Inferred from indirect evidence" },
  unknown: { tone: "neutral", label: "Unknown", hint: "Not enough evidence" },
};

export function ConfidenceBadge({ level }: { level: ConfidenceLevel }) {
  const c = CONFIDENCE[level];
  return (
    <Badge tone={c.tone} title={c.hint}>
      {c.label}
    </Badge>
  );
}
