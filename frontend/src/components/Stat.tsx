import type { ReactNode } from "react";
import { Link } from "react-router";
import type { Icon as PhIcon } from "@phosphor-icons/react";
import { Card, CardContent } from "./ui/card";
import { cn } from "@/lib/utils";

const TINT = {
  primary: "bg-primary/12 text-primary",
  success: "bg-success/12 text-success",
  warning: "bg-warning/15 text-warning",
  danger: "bg-destructive/12 text-destructive",
} as const;

/** Overview stat card: tinted icon tile, label, big number. Compact on phones. */
export function Stat({ label, value, sub, icon: Icon, to, tone }: { label: string; value: ReactNode; sub?: ReactNode; icon: PhIcon; to?: string; tone?: "danger" | "warning" | "success" | "primary" }) {
  const body = (
    <Card className={cn("h-full gap-0 py-0 shadow-sm transition-shadow", to && "hover:shadow-md")}>
      <CardContent className="flex items-center gap-3 p-3 sm:gap-4 sm:p-5">
        <div className={cn("flex size-10 shrink-0 items-center justify-center rounded-xl sm:size-12", TINT[tone ?? "primary"])}>
          <Icon weight="duotone" className="size-5 sm:size-6" aria-hidden />
        </div>
        <div className="min-w-0">
          <p className="truncate text-xs font-medium text-muted-foreground sm:text-sm">{label}</p>
          <p className="break-words text-xl font-semibold tracking-tight tabular-nums sm:text-2xl">{value}</p>
          {sub && <div className="truncate text-xs text-muted-foreground">{sub}</div>}
        </div>
      </CardContent>
    </Card>
  );
  return to ? <Link to={to} className="block min-h-11 rounded-xl focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none">{body}</Link> : body;
}

export interface CurrencyTotal { currency: string; amount: string; count: number }
