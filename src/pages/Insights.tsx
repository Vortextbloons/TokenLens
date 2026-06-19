import { useEffect, useState } from "react";
import { useFilterObject } from "@/stores/filter";
import { useDataRevision } from "@/stores/dataRevision";
import { getAnomalyHighlights, getCacheEfficiency, getContextUtilization } from "@/lib/tauri";
import type {
  AnomalyHighlight,
  CacheEfficiencyReport,
  ContextUtilizationReport,
} from "@/types/contracts";
import { PageHeader } from "@/components/layout/PageHeader";
import { Card, CardContent, CardHeader, CardTitle, Skeleton, Badge, Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/primitives";
import { StatCard } from "@/components/cards/StatCard";
import { formatNumber, formatPercent, formatUsd } from "@/lib/utils";
import { AlertTriangle, Database, ShieldAlert, Sparkles } from "lucide-react";

function ratioLabel(n: number): string {
  return `${n.toFixed(2)}x`;
}

export function Insights() {
  const filter = useFilterObject();
  const dataRevision = useDataRevision((s) => s.revision);
  const [anomalies, setAnomalies] = useState<AnomalyHighlight[] | null>(null);
  const [cache, setCache] = useState<CacheEfficiencyReport | null>(null);
  const [context, setContext] = useState<ContextUtilizationReport | null>(null);

  useEffect(() => {
    setAnomalies(null);
    setCache(null);
    setContext(null);
    Promise.all([
      getAnomalyHighlights(filter),
      getCacheEfficiency(filter),
      getContextUtilization(filter),
    ])
      .then(([a, c, ctx]) => {
        setAnomalies(a);
        setCache(c);
        setContext(ctx);
      })
      .catch(() => {
        setAnomalies([]);
        setCache(null);
        setContext(null);
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [JSON.stringify(filter), dataRevision]);

  const anomalyCount = anomalies?.length ?? 0;
  const cacheSavings = cache?.series.reduce((a, b) => a + b.cache_savings_usd, 0) ?? 0;
  const cacheReadTokens = cache?.series.reduce((a, b) => a + b.cache_read_tokens, 0) ?? 0;
  const cacheWriteTokens = cache?.series.reduce((a, b) => a + b.cache_write_tokens, 0) ?? 0;
  const contextTop = context?.sessions[0];
  const over80 = context?.trend.reduce((a, b) => a + b.sessions_over_80, 0) ?? 0;

  return (
    <div className="animate-fade-in space-y-6">
      <PageHeader
        title="Insights"
        description="Anomalies, cache efficiency, and context pressure in one place."
      />

      <div className="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-4 gap-4">
        <StatCard title="Anomalies" value={formatNumber(anomalyCount)} hint="spikes flagged" icon={AlertTriangle} />
        <StatCard title="Cache savings" value={formatUsd(cacheSavings)} hint="USD from cache hits" icon={Database} />
        <StatCard title="Cache reads" value={formatNumber(cacheReadTokens)} hint="tokens read from cache" icon={Sparkles} />
        <StatCard title="80%+ sessions" value={formatNumber(over80)} hint="sessions near limit" icon={ShieldAlert} />
      </div>

      <div className="grid grid-cols-1 xl:grid-cols-2 gap-4">
        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">Anomaly highlights</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            {anomalies === null ? (
              <Skeleton className="h-64" />
            ) : anomalies.length === 0 ? (
              <div className="text-sm text-muted-foreground py-10 text-center">No spikes above the rolling median.</div>
            ) : (
              anomalies.map((a) => (
                <div key={`${a.kind}-${a.session_id ?? a.date ?? a.label}`} className="rounded-md border p-3 text-sm space-y-2">
                  <div className="flex items-start gap-2">
                    <Badge variant={a.kind === "day" ? "warning" : "secondary"}>{a.kind}</Badge>
                    <div className="flex-1 min-w-0">
                      <div className="font-medium truncate">{a.label}</div>
                      <div className="text-xs text-muted-foreground">{a.reason} · {ratioLabel(a.ratio)} above median</div>
                    </div>
                  </div>
                  <div className="grid grid-cols-2 sm:grid-cols-4 gap-2 text-xs text-muted-foreground">
                    <div>Tokens: <span className="text-foreground tabular-nums">{formatNumber(a.total_tokens)}</span></div>
                    <div>Baseline: <span className="text-foreground tabular-nums">{formatNumber(a.baseline_tokens)}</span></div>
                    <div>Peak context: <span className="text-foreground tabular-nums">{formatPercent(a.peak_context_pct)}</span></div>
                    <div>Events: <span className="text-foreground tabular-nums">{formatNumber(a.event_count)}</span></div>
                  </div>
                </div>
              ))
            )}
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">Context pressure</CardTitle>
          </CardHeader>
          <CardContent className="space-y-3">
            {context === null ? (
              <Skeleton className="h-64" />
            ) : context.sessions.length === 0 ? (
              <div className="text-sm text-muted-foreground py-10 text-center">No sessions with known context limits.</div>
            ) : (
              <>
                <div className="grid grid-cols-2 gap-3 text-sm">
                  <div>
                    <div className="text-xs text-muted-foreground">Peak session</div>
                    <div className="font-medium truncate">{contextTop?.label ?? "—"}</div>
                  </div>
                  <div>
                    <div className="text-xs text-muted-foreground">Utilization</div>
                    <div className="font-medium tabular-nums">{contextTop ? formatPercent(contextTop.utilization_pct) : "—"}</div>
                  </div>
                </div>
                <Table>
                  <TableHeader>
                    <TableRow>
                      <TableHead>Date</TableHead>
                      <TableHead className="text-right">Avg</TableHead>
                      <TableHead className="text-right">Max</TableHead>
                      <TableHead className="text-right">80%+</TableHead>
                    </TableRow>
                  </TableHeader>
                  <TableBody>
                    {context.trend.map((row) => (
                      <TableRow key={row.date}>
                        <TableCell>{row.date}</TableCell>
                        <TableCell className="text-right tabular-nums">{formatPercent(row.avg_utilization_pct)}</TableCell>
                        <TableCell className="text-right tabular-nums">{formatPercent(row.max_utilization_pct)}</TableCell>
                        <TableCell className="text-right tabular-nums">{formatNumber(row.sessions_over_80)}</TableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </>
            )}
          </CardContent>
        </Card>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm font-medium">Cache efficiency</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          {cache === null ? (
            <Skeleton className="h-64" />
          ) : (
            <>
              <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
                <div>
                  <div className="text-sm font-medium mb-2">By provider</div>
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Provider</TableHead>
                        <TableHead className="text-right">Savings</TableHead>
                        <TableHead className="text-right">Read / Write</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {cache.by_provider.slice(0, 8).map((row) => (
                        <TableRow key={row.key}>
                          <TableCell>{row.key}</TableCell>
                          <TableCell className="text-right tabular-nums">{formatUsd(row.cache_savings_usd)}</TableCell>
                          <TableCell className="text-right tabular-nums">
                            {formatNumber(row.cache_read_tokens)} / {formatNumber(row.cache_write_tokens)}
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
                <div>
                  <div className="text-sm font-medium mb-2">By model</div>
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead>Model</TableHead>
                        <TableHead className="text-right">Savings</TableHead>
                        <TableHead className="text-right">Read / Write</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {cache.by_model.slice(0, 8).map((row) => (
                        <TableRow key={row.key}>
                          <TableCell className="truncate max-w-[220px]">{row.key}</TableCell>
                          <TableCell className="text-right tabular-nums">{formatUsd(row.cache_savings_usd)}</TableCell>
                          <TableCell className="text-right tabular-nums">
                            {formatNumber(row.cache_read_tokens)} / {formatNumber(row.cache_write_tokens)}
                          </TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </div>
              </div>

              <Table>
                <TableHeader>
                  <TableRow>
                    <TableHead>Date</TableHead>
                    <TableHead className="text-right">Read</TableHead>
                    <TableHead className="text-right">Write</TableHead>
                    <TableHead className="text-right">Savings</TableHead>
                    <TableHead className="text-right">Read / Write</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {cache.series.map((row) => (
                    <TableRow key={row.date}>
                      <TableCell>{row.date}</TableCell>
                      <TableCell className="text-right tabular-nums">{formatNumber(row.cache_read_tokens)}</TableCell>
                      <TableCell className="text-right tabular-nums">{formatNumber(row.cache_write_tokens)}</TableCell>
                      <TableCell className="text-right tabular-nums">{formatUsd(row.cache_savings_usd)}</TableCell>
                      <TableCell className="text-right tabular-nums">
                        {row.cache_write_tokens > 0 ? ratioLabel(row.cache_read_tokens / row.cache_write_tokens) : "—"}
                      </TableCell>
                    </TableRow>
                  ))}
                </TableBody>
              </Table>
            </>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
