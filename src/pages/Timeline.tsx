import { useEffect, useMemo, useState } from "react";
import { useFilterObject } from "@/stores/filter";
import { useDataRevision } from "@/stores/dataRevision";
import { getUsageTimeseries, listEvents } from "@/lib/tauri";
import type { TimeseriesPoint, UsageEvent } from "@/types/contracts";
import { PageHeader } from "@/components/layout/PageHeader";
import { Card, CardContent, CardHeader, CardTitle, Skeleton } from "@/components/ui/primitives";
import { TokensAreaChart, CostLineChart } from "@/charts";
import { formatNumber } from "@/lib/utils";
import { EmptyState } from "@/components/layout/PageHeader";

export function Timeline() {
  const filter = useFilterObject();
  const dataRevision = useDataRevision((s) => s.revision);
  const [series, setSeries] = useState<TimeseriesPoint[]>([]);
  const [events, setEvents] = useState<UsageEvent[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading(true);
    Promise.all([
      getUsageTimeseries(filter),
      listEvents({ ...filter, limit: 1000 }),
    ]).then(([s, e]) => { setSeries(s); setEvents(e); }).finally(() => setLoading(false));
    // eslint-disable-next-line
  }, [JSON.stringify(filter), dataRevision]);

  // Heatmap: 7 (dow) x 24 (hour)
  const heatmap = useMemo(() => {
    const m: number[][] = Array.from({ length: 7 }, () => Array(24).fill(0));
    for (const e of events) {
      const d = new Date(e.timestamp);
      m[d.getDay()][d.getHours()] += e.total_tokens;
    }
    let max = 0;
    for (const r of m) for (const v of r) if (v > max) max = v;
    return { m, max };
  }, [events]);

  const days = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

  return (
    <div className="animate-fade-in">
      <PageHeader title="Timeline" description="When do you use the most tokens? Find spikes, idle time, and trends." />
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4 mb-4">
        <Card>
          <CardHeader><CardTitle className="text-sm font-medium">Tokens over time</CardTitle></CardHeader>
          <CardContent>
            {loading ? <Skeleton className="h-64" /> : series.length > 0 ? <TokensAreaChart data={series} /> : <EmptyState title="No data" />}
          </CardContent>
        </Card>
        <Card>
          <CardHeader><CardTitle className="text-sm font-medium">Cost over time</CardTitle></CardHeader>
          <CardContent>
            {loading ? <Skeleton className="h-64" /> : series.length > 0 ? <CostLineChart data={series} /> : <EmptyState title="No data" />}
          </CardContent>
        </Card>
      </div>
      <Card>
        <CardHeader>
          <CardTitle className="text-sm font-medium">Activity heatmap (day of week × hour)</CardTitle>
        </CardHeader>
        <CardContent>
          {loading ? <Skeleton className="h-64" /> : (
            <div className="space-y-1.5">
              <div className="grid grid-cols-[40px_repeat(24,1fr)] gap-1 text-[10px] text-muted-foreground">
                <div></div>
                {Array.from({ length: 24 }, (_, h) => <div key={h} className="text-center">{h}</div>)}
              </div>
              {heatmap.m.map((row, di) => (
                <div key={di} className="grid grid-cols-[40px_repeat(24,1fr)] gap-1 items-center">
                  <div className="text-[10px] text-muted-foreground">{days[di]}</div>
                  {row.map((v, hi) => {
                    const intensity = heatmap.max > 0 ? v / heatmap.max : 0;
                    return (
                      <div
                        key={hi}
                        className="h-5 rounded-sm transition-colors"
                        title={`${days[di]} ${hi}:00 — ${formatNumber(v)} tokens`}
                        style={{
                          backgroundColor: `rgba(45, 212, 191, ${0.05 + intensity * 0.85})`,
                        }}
                      />
                    );
                  })}
                </div>
              ))}
              <div className="flex items-center gap-2 mt-3 text-[10px] text-muted-foreground">
                <span>less</span>
                <div className="flex gap-0.5">
                  {[0.1, 0.3, 0.5, 0.7, 0.9].map((a) => (
                    <div key={a} className="h-3 w-4 rounded-sm" style={{ backgroundColor: `rgba(45, 212, 191, ${a})` }} />
                  ))}
                </div>
                <span>more</span>
                <span className="ml-auto">peak: {formatNumber(heatmap.max)} tokens</span>
              </div>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
