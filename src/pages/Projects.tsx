import { useEffect, useState } from "react";
import { useFilterObject } from "@/stores/filter";
import { getBreakdown, getUsageTimeseries, getOverviewStats } from "@/lib/tauri";
import type { Breakdown, TimeseriesPoint, OverviewStats } from "@/types/contracts";
import { PageHeader } from "@/components/layout/PageHeader";
import { Card, CardContent, CardHeader, CardTitle, Skeleton } from "@/components/ui/primitives";
import { CostLineChart, StackedBarChart } from "@/charts";
import { formatNumber, formatUsd } from "@/lib/utils";
import { EmptyState } from "@/components/layout/PageHeader";

export function Projects() {
  const filter = useFilterObject();
  const [data, setData] = useState<Breakdown[] | null>(null);
  const [series, setSeries] = useState<TimeseriesPoint[]>([]);
  const [stats, setStats] = useState<OverviewStats | null>(null);

  useEffect(() => {
    setData(null);
    Promise.all([
      getBreakdown(filter, "project"),
      getUsageTimeseries(filter),
      getOverviewStats(filter),
    ]).then(([b, s, st]) => { setData(b); setSeries(s); setStats(st); });
    // eslint-disable-next-line
  }, [JSON.stringify(filter)]);

  return (
    <div className="animate-fade-in">
      <PageHeader title="Projects" description="Token and cost grouped by project, workspace, or repo." />
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-4 mb-4">
        <Card>
          <CardContent className="pt-5">
            <div className="text-xs text-muted-foreground">Projects</div>
            <div className="text-2xl font-semibold tabular-nums">{data?.length ?? "—"}</div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-5">
            <div className="text-xs text-muted-foreground">Total tokens</div>
            <div className="text-2xl font-semibold tabular-nums text-teal-500">{formatNumber(stats?.tokens_month ?? 0)}</div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-5">
            <div className="text-xs text-muted-foreground">Total cost</div>
            <div className="text-2xl font-semibold tabular-nums text-violet-500">{formatUsd(stats?.cost_month_usd ?? 0)}</div>
          </CardContent>
        </Card>
      </div>
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        <Card>
          <CardHeader><CardTitle className="text-sm font-medium">Cost over time</CardTitle></CardHeader>
          <CardContent>
            {series.length > 0 ? <CostLineChart data={series} /> : <EmptyState title="No data" />}
          </CardContent>
        </Card>
        <Card>
          <CardHeader><CardTitle className="text-sm font-medium">Token composition</CardTitle></CardHeader>
          <CardContent>
            {series.length > 0 ? <StackedBarChart data={series} /> : <EmptyState title="No data" />}
          </CardContent>
        </Card>
        <Card className="lg:col-span-2">
          <CardHeader><CardTitle className="text-sm font-medium">Projects</CardTitle></CardHeader>
          <CardContent className="p-0">
            {data === null ? <Skeleton className="h-32 m-5" /> : data.length === 0 ? <EmptyState title="No projects" description="Project detection improves as more events are collected." /> : (
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-muted-foreground text-xs">
                    <th className="text-left p-3">Project</th>
                    <th className="text-right p-3">Tokens</th>
                    <th className="text-right p-3">Sessions</th>
                    <th className="text-right p-3">Cost</th>
                  </tr>
                </thead>
                <tbody>
                  {data.map((d) => (
                    <tr key={d.key} className="border-b hover:bg-muted/30">
                      <td className="p-3 font-medium">{d.key}</td>
                      <td className="p-3 text-right tabular-nums">{formatNumber(d.total_tokens)}</td>
                      <td className="p-3 text-right tabular-nums">{d.sessions_count}</td>
                      <td className="p-3 text-right tabular-nums">{formatUsd(d.cost_usd)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
