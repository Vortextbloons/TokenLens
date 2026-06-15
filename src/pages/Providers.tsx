import { useEffect, useState } from "react";
import { useFilterObject } from "@/stores/filter";
import { useDataRevision } from "@/stores/dataRevision";
import { getBreakdown } from "@/lib/tauri";
import type { Breakdown } from "@/types/contracts";
import { PageHeader } from "@/components/layout/PageHeader";
import { Card, CardContent, CardHeader, CardTitle, Skeleton } from "@/components/ui/primitives";
import { ProviderDonut } from "@/charts";
import { formatNumber, formatUsd } from "@/lib/utils";
import { EmptyState } from "@/components/layout/PageHeader";

export function Providers() {
  const filter = useFilterObject();
  const dataRevision = useDataRevision((s) => s.revision);
  const [data, setData] = useState<Breakdown[] | null>(null);

  useEffect(() => {
    setData(null);
    getBreakdown(filter, "provider").then(setData).catch(() => setData([]));
    // eslint-disable-next-line
  }, [JSON.stringify(filter), dataRevision]);

  return (
    <div className="animate-fade-in">
      <PageHeader title="Providers" description="Share of tokens and cost by provider." />
      <div className="grid grid-cols-1 xl:grid-cols-12 gap-4">
        <Card className="xl:col-span-5">
          <CardHeader><CardTitle className="text-sm font-medium">Share of tokens</CardTitle></CardHeader>
          <CardContent className="min-h-[220px]">
            {data === null ? <Skeleton className="h-44" /> : data.length > 0 ? <ProviderDonut data={data} /> : <EmptyState title="No data" />}
          </CardContent>
        </Card>
        <Card className="xl:col-span-7">
          <CardHeader><CardTitle className="text-sm font-medium">Provider totals</CardTitle></CardHeader>
          <CardContent className="p-0">
            {data === null ? <Skeleton className="h-44 m-5" /> : (
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-muted-foreground text-xs">
                    <th className="text-left p-3">Provider</th>
                    <th className="text-right p-3">Tokens</th>
                    <th className="text-right p-3">Cost</th>
                    <th className="text-right p-3">Sessions</th>
                  </tr>
                </thead>
                <tbody>
                  {data.map((d) => (
                    <tr key={d.key} className="border-b hover:bg-muted/30">
                      <td className="p-3 font-medium">{d.key}</td>
                      <td className="p-3 text-right tabular-nums">{formatNumber(d.total_tokens)}</td>
                      <td className="p-3 text-right tabular-nums">{formatUsd(d.cost_usd)}</td>
                      <td className="p-3 text-right tabular-nums">{d.sessions_count}</td>
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
