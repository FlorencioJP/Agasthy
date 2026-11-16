import type { Availability } from "@agasthy/shared-types";
import { cn } from "@agasthy/ui";
import type { ReactNode } from "react";
import { unavailableReason } from "../lib/format";

/**
 * Render an Availability<T>. Missing values are shown as an explicit,
 * explained state — never as 0 or an empty string.
 */
export function Avail<T>({
  value,
  render,
  className,
}: {
  value: Availability<T> | undefined | null;
  render: (v: T) => ReactNode;
  className?: string;
}) {
  if (!value) return <span className={cn("text-faint", className)}>—</span>;
  if (value.status === "available") return <>{render(value.value)}</>;
  const label =
    value.status === "permission_denied" ? "Access denied" : value.status === "unsupported" ? "Unsupported" : "Unavailable";
  return (
    <span
      title={unavailableReason(value)}
      className={cn(
        "cursor-help text-[12px] italic decoration-dotted underline-offset-2 hover:underline",
        value.status === "permission_denied" ? "text-warning" : "text-faint",
        className,
      )}
    >
      {label}
    </span>
  );
}
