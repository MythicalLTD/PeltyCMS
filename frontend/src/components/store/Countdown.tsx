import { useEffect, useState } from "react";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

/** Seconds left until `endsAt` (0 once over), ticking every second. */
export function useSecondsLeft(endsAt: string): number {
  const end = new Date(endsAt).getTime();
  const calc = () => (Number.isFinite(end) ? Math.max(0, Math.floor((end - Date.now()) / 1000)) : 0);
  const [left, setLeft] = useState(calc);
  useEffect(() => {
    setLeft(calc());
    const id = window.setInterval(() => setLeft(calc()), 1000);
    return () => window.clearInterval(id);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [end]);
  return left;
}

/** Live d / h / m / s boxes; nothing once the time is over. */
export function Countdown({ endsAt, className, size = "md" }: { endsAt: string; className?: string; size?: "sm" | "md" }) {
  const { t } = useI18n();
  const left = useSecondsLeft(endsAt);
  if (left <= 0) return null;
  const parts = [
    [Math.floor(left / 86400), t("countdown.days")],
    [Math.floor((left % 86400) / 3600), t("countdown.hours")],
    [Math.floor((left % 3600) / 60), t("countdown.minutes")],
    [left % 60, t("countdown.seconds")],
  ] as const;
  return (
    <div className={cn("flex items-center gap-1.5", className)} role="timer" aria-live="off" data-testid="countdown">
      {parts.map(([n, label]) => (
        <span key={label} className={cn("store-countdown-cell flex flex-col items-center rounded-md border bg-background tabular-nums", size === "sm" ? "min-w-10 px-1.5 py-1" : "min-w-14 px-2 py-1.5")}>
          <b className={size === "sm" ? "text-sm leading-none" : "text-xl leading-none"}>{String(n).padStart(2, "0")}</b>
          <span className="mt-0.5 text-[0.625rem] uppercase tracking-wider text-muted-foreground">{label}</span>
        </span>
      ))}
    </div>
  );
}
