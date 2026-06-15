import { useEffect, useState } from "react";
import { useFilterObject } from "@/stores/filter";
import { useDataRevision } from "@/stores/dataRevision";
import { getBreakdown } from "@/lib/tauri";
import type { Breakdown } from "@/types/contracts";
import { PageHeader } from "@/components/layout/PageHeader";
import { Card, CardContent, CardHeader, CardTitle, Skeleton } from "@/components/ui/primitives";
import { ModelBarChart } from "@/charts";
import { formatNumber, formatUsd } from "@/lib/utils";
import { EmptyState } from "@/components/layout/PageHeader";

export function Models() {
  const filter = useFilterObject();
  const dataRevision = useDataRevision((s) => s.revision);
  const [data, setData] = useState<Breakdown[] | null>(null);

  useEffect(() => {
    setData(null);
    getBreakdown(filter, "model").then(setData).catch(() => setData([]));
    // eslint-disable-next-line
  }, [JSON.stringify(filter), dataRevision]);

  return (
    <div className="animate-fade-in">
      <PageHeader title="Models" description="Per-model token and cost breakdown." />
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        <Card>
          <CardHeader><CardTitle className="text-sm font-medium">Tokens by model</CardTitle></CardHeader>
          <CardContent>
            {data === null ? <Skeleton className="h-72" /> : data.length > 0 ? <ModelBarChart data={data} /> : <EmptyState title="No data" />}
          </CardContent>
        </Card>
        <Card>
          <CardHeader><CardTitle className="text-sm font-medium">Cost by model</CardTitle></CardHeader>
          <CardContent>
            {data === null ? <Skeleton className="h-72" /> : data.length > 0 ? (
              <div className="space-y-2">
                {data.map((d) => (
                  <div key={d.key} className="flex items-center gap-3">
                    <div className="text-sm flex-1 truncate">{d.key}</div>
                    <div className="w-1/2 h-2 bg-muted rounded-full overflow-hidden">
                      <div
                        className="h-full bg-gradient-to-r from-teal-500 to-cyan-500"
                        style={{ width: `${data[0].total_tokens > 0 ? (d.total_tokens / data[0].total_tokens) * 100 : 0}%` }}
                      />
                    </div>
                    <div className="text-xs tabular-nums w-16 text-right">{formatUsd(d.cost_usd)}</div>
                  </div>
                ))}
              </div>
            ) : <EmptyState title="No data" />}
          </CardContent>
        </Card>
        <Card className="lg:col-span-2">
          <CardHeader><CardTitle className="text-sm font-medium">Top models — table</CardTitle></CardHeader>
          <CardContent className="p-0">
            {data === null ? <Skeleton className="h-72 m-5" /> : (
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-muted-foreground text-xs">
                    <th className="text-left p-3">Model</th>
                    <th className="text-right p-3">Input</th>
                    <th className="text-right p-3">Output</th>
                    <th className="text-right p-3">Total tokens</th>
                    <th className="text-right p-3">Cost</th>
                  </tr>
                </thead>
                <tbody>
                  {data.map((d) => (
                    <tr key={d.key} className="border-b hover:bg-muted/30">
                      <td className="p-3 font-medium">{d.key}</td>
                      <td className="p-3 text-right tabular-nums">{formatNumber(d.input_tokens)}</td>
                      <td className="p-3 text-right tabular-nums">{formatNumber(d.output_tokens)}</td>
                      <td className="p-3 text-right tabular-nums">{formatNumber(d.total_tokens)}</td>
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
