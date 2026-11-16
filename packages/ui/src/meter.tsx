import { cn } from "./cn";

export interface MeterProps {
  /** 0..1. `null` renders an empty, hatched track meaning "unavailable". */
  value: number | null;
  label: string;
  warnAt?: number;
  dangerAt?: number;
  className?: string;
}

export function Meter({ value, label, warnAt = 0.75, dangerAt = 0.9, className }: MeterProps) {
  const v = value === null ? null : Math.min(1, Math.max(0, value));
  const tone = v === null ? "" : v >= dangerAt ? "bg-danger" : v >= warnAt ? "bg-warning" : "bg-accent";
  return (
    <div
      role="meter"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={v === null ? undefined : Math.round(v * 100)}
      aria-valuetext={v === null ? "unavailable" : `${Math.round(v * 100)}%`}
      className={cn("h-1.5 w-full overflow-hidden rounded-full bg-raised", v === null && "meter-unavailable", className)}
    >
      {v !== null ? <div className={cn("h-full rounded-full transition-[width] duration-500", tone)} style={{ width: `${v * 100}%` }} /> : null}
    </div>
  );
}
