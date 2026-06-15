import { useEffect, useState } from "react";
import { useFilterObject } from "@/stores/filter";
import { getBreakdown, getOverviewStats, getUsageTimeseries } from "@/lib/tauri";
import type { Breakdown, OverviewStats, TimeseriesPoint } from "@/types/contracts";
import { PageHeader } from "@/components/layout/PageHeader";
import { Card, CardContent, CardHeader, CardTitle, Skeleton, Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/primitives";
import { CostLineChart } from "@/charts";
import { formatNumber, formatUsd } from "@/lib/utils";
import { EmptyState } from "@/components/layout/PageHeader";
import { TrendingUp, AlertTriangle } from "lucide-react";

export function Costs() {
  const filter = useFilterObject();
  const [byProvider, setByProvider] = useState<Breakdown[] | null>(null);
  const [byModel, setByModel] = useState<Breakdown[] | null>(null);
  const [series, setSeries] = useState<TimeseriesPoint[]>([]);
  const [stats, setStats] = useState<OverviewStats | null>(null);
  const [tab, setTab] = useState("trend");

  useEffect(() => {
    Promise.all([
      getBreakdown(filter, "provider"),
      getBreakdown(filter, "model"),
      getUsageTimeseries(filter),
      getOverviewStats(filter),
    ]).then(([p, m, s, st]) => {
      setByProvider(p); setByModel(m); setSeries(s); setStats(st);
    });
    // eslint-disable-next-line
  }, [JSON.stringify(filter)]);

  // Simple linear forecast based on last 14 days
  const last14 = series.slice(-14);
  let forecast = 0;
  if (last14.length >= 2) {
    const xs = last14.map((_, i) => i);
    const ys = last14.map((p) => p.cost_usd);
    const n = xs.length;
    const sumX = xs.reduce((a, b) => a + b, 0);
    const sumY = ys.reduce((a, b) => a + b, 0);
    const sumXY = xs.reduce((a, b, i) => a + b * ys[i], 0);
    const sumXX = xs.reduce((a, b) => a + b * b, 0);
    const slope = (n * sumXY - sumX * sumY) / (n * sumXX - sumX * sumX);
    const intercept = (sumY - slope * sumX) / n;
    const lastX = xs[xs.length - 1];
    const daysToProject = 30 - (series.length - last14.length);
    forecast = Math.max(0, slope * (lastX + daysToProject) + intercept);
  }

  return (
    <div className="animate-fade-in">
      <PageHeader title="Costs" description="Daily, weekly, monthly, and forecasted costs." />
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4 mb-4">
        <Card>
          <CardContent className="pt-5">
            <div className="text-xs text-muted-foreground">Cost today</div>
            <div className="text-2xl font-semibold tabular-nums text-violet-500">{formatUsd(stats?.cost_today_usd ?? 0)}</div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-5">
            <div className="text-xs text-muted-foreground">Last 7 days</div>
            <div className="text-2xl font-semibold tabular-nums">{formatUsd(stats?.cost_week_usd ?? 0)}</div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-5">
            <div className="text-xs text-muted-foreground">Last 30 days</div>
            <div className="text-2xl font-semibold tabular-nums">{formatUsd(stats?.cost_month_usd ?? 0)}</div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-5">
            <div className="text-xs text-muted-foreground flex items-center gap-1">
              <TrendingUp className="h-3 w-3" /> 30-day forecast
            </div>
            <div className="text-2xl font-semibold tabular-nums flex items-center gap-2">
              {formatUsd(forecast)}
            </div>
            <div className="text-[10px] text-muted-foreground">linear regression · last 14 days</div>
          </CardContent>
        </Card>
      </div>

      <Tabs value={tab} onValueChange={setTab}>
        <TabsList>
          <TabsTrigger value="trend">Trend</TabsTrigger>
          <TabsTrigger value="provider">By provider</TabsTrigger>
          <TabsTrigger value="model">By model</TabsTrigger>
        </TabsList>
        <TabsContent value="trend">
          <Card>
            <CardHeader><CardTitle className="text-sm font-medium">Cost over time</CardTitle></CardHeader>
            <CardContent>
              {series.length > 0 ? <CostLineChart data={series} /> : <EmptyState title="No data" />}
            </CardContent>
          </Card>
        </TabsContent>
        <TabsContent value="provider">
          <Card>
            <CardContent className="p-0">
              {byProvider === null ? <Skeleton className="h-48 m-5" /> : (
                <table className="w-full text-sm">
                  <thead><tr className="border-b text-muted-foreground text-xs">
                    <th className="text-left p-3">Provider</th>
                    <th className="text-right p-3">Cost</th>
                    <th className="text-right p-3">Tokens</th>
                    <th className="text-right p-3">% of total</th>
                  </tr></thead>
                  <tbody>
                    {byProvider.map((d) => (
                      <tr key={d.key} className="border-b hover:bg-muted/30">
                        <td className="p-3 font-medium">{d.key}</td>
                        <td className="p-3 text-right tabular-nums">{formatUsd(d.cost_usd)}</td>
                        <td className="p-3 text-right tabular-nums">{formatNumber(d.total_tokens)}</td>
                        <td className="p-3 text-right tabular-nums">
                          {stats && stats.cost_month_usd > 0 ? ((d.cost_usd / stats.cost_month_usd) * 100).toFixed(1) : "0"}%
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </CardContent>
          </Card>
        </TabsContent>
        <TabsContent value="model">
          <Card>
            <CardContent className="p-0">
              {byModel === null ? <Skeleton className="h-48 m-5" /> : (
                <table className="w-full text-sm">
                  <thead><tr className="border-b text-muted-foreground text-xs">
                    <th className="text-left p-3">Model</th>
                    <th className="text-right p-3">Cost</th>
                    <th className="text-right p-3">Tokens</th>
                    <th className="text-right p-3">$ / 1K tokens</th>
                  </tr></thead>
                  <tbody>
                    {byModel.map((d) => (
                      <tr key={d.key} className="border-b hover:bg-muted/30">
                        <td className="p-3 font-medium">{d.key}</td>
                        <td className="p-3 text-right tabular-nums">{formatUsd(d.cost_usd)}</td>
                        <td className="p-3 text-right tabular-nums">{formatNumber(d.total_tokens)}</td>
                        <td className="p-3 text-right tabular-nums text-muted-foreground">
                          {d.total_tokens > 0 ? formatUsd((d.cost_usd / d.total_tokens) * 1000) : "—"}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>

      <Card className="mt-4 border-amber-500/40 bg-amber-500/5">
        <CardContent className="py-4 flex items-start gap-3">
          <AlertTriangle className="h-4 w-4 text-amber-500 mt-0.5" />
          <div className="text-sm">
            <div className="font-medium">Cost numbers are estimates</div>
            <div className="text-muted-foreground text-xs">Token counts come from your logs; costs are computed from your pricing table. Set exact per-model rates in Settings → Pricing.</div>
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
