import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/primitives";
import { cn, formatNumber, formatPercent, formatUsd } from "@/lib/utils";
import { ArrowDownRight, ArrowUpRight, type LucideIcon } from "lucide-react";

interface StatCardProps {
  title: string;
  value: string;
  hint?: string;
  delta?: { value: string; positive: boolean } | null;
  icon?: LucideIcon;
  emphasize?: "tokens" | "cost" | "neutral";
}

export function StatCard({ title, value, hint, delta, icon: Icon, emphasize = "neutral" }: StatCardProps) {
  return (
    <Card>
      <CardHeader className="pb-2">
        <div className="flex items-center justify-between">
          <CardTitle className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
            {title}
          </CardTitle>
          {Icon ? <Icon className="h-3.5 w-3.5 text-muted-foreground" /> : null}
        </div>
      </CardHeader>
      <CardContent>
        <div className={cn(
          "text-2xl font-semibold tracking-tight tabular-nums",
          emphasize === "tokens" && "text-teal-500",
          emphasize === "cost" && "text-violet-500"
        )}>
          {value}
        </div>
        <div className="flex items-center gap-2 mt-1.5">
          {delta ? (
            <span className={cn(
              "inline-flex items-center gap-0.5 text-xs font-medium",
              delta.positive ? "text-emerald-500" : "text-rose-500"
            )}>
              {delta.positive ? <ArrowUpRight className="h-3 w-3" /> : <ArrowDownRight className="h-3 w-3" />}
              {delta.value}
            </span>
          ) : null}
          {hint ? <span className="text-xs text-muted-foreground">{hint}</span> : null}
        </div>
      </CardContent>
    </Card>
  );
}

export function TokensCard({ value, hint, delta }: { value: number; hint?: string; delta?: { value: string; positive: boolean } | null }) {
  return <StatCard title="Tokens" value={formatNumber(value)} hint={hint} delta={delta} emphasize="tokens" />;
}
export function CostCard({ value, hint, delta }: { value: number; hint?: string; delta?: { value: string; positive: boolean } | null }) {
  return <StatCard title="Cost" value={formatUsd(value)} hint={hint} delta={delta} emphasize="cost" />;
}
export function PercentCard({ title, value, hint }: { title: string; value: number; hint?: string }) {
  return <StatCard title={title} value={formatPercent(value)} hint={hint} />;
}
