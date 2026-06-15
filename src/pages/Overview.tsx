import { useEffect, useState } from "react";
import { useFilterObject } from "@/stores/filter";
import { useDataRevision } from "@/stores/dataRevision";
import { getOverviewStats, getUsageTimeseries, getBreakdown, listAlerts, evaluateBudgets } from "@/lib/tauri";
import type { OverviewStats, TimeseriesPoint, Breakdown } from "@/types/contracts";
import { PageHeader, EmptyState } from "@/components/layout/PageHeader";
import { StatCard, TokensCard, CostCard } from "@/components/cards/StatCard";
import { Card, CardContent, CardHeader, CardTitle, Skeleton } from "@/components/ui/primitives";
import { TokensAreaChart, CostLineChart, ModelBarChart, ProviderDonut } from "@/charts";
import { Cpu, DollarSign, MessageSquare, TrendingUp, AlertTriangle, Sparkles, Database, X } from "lucide-react";
import { formatNumber, formatPercent, formatUsd, errorMessage, formatDelta } from "@/lib/utils";
import { useFilter } from "@/stores/filter";
import { Button } from "@/components/ui/primitives";
import { isTauri, generateSampleData, purgeSampleData } from "@/lib/tauri";
import { toast } from "@/stores/toast";

interface AlertRow {
  id: number;
  alert_type: string;
  severity: string;
  title: string;
  message: string;
  created_at: string;
  acknowledged_at: string | null;
}

export function Overview() {
  const filter = useFilterObject();
  const range = useFilter((s) => s.range);
  const dataRevision = useDataRevision((s) => s.revision);
  const [stats, setStats] = useState<OverviewStats | null>(null);
  const [series, setSeries] = useState<TimeseriesPoint[]>([]);
  const [models, setModels] = useState<Breakdown[]>([]);
  const [providers, setProviders] = useState<Breakdown[]>([]);
  const [alerts, setAlerts] = useState<AlertRow[]>([]);
  const [loading, setLoading] = useState(true);

  const reload = async () => {
    setLoading(true);
    try {
      const [s, ts, mb, pb] = await Promise.all([
        getOverviewStats(filter),
        getUsageTimeseries(filter),
        getBreakdown(filter, "model"),
        getBreakdown(filter, "provider"),
      ]);
      setStats(s);
      setSeries(ts);
      setModels(mb);
      setProviders(pb);
      try {
        if (isTauri) await evaluateBudgets();
        setAlerts(await listAlerts(10));
      } catch { /* ignore */ }
    } catch (e) {
      toast({ title: "Failed to load overview", description: errorMessage(e), variant: "destructive" });
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => { reload(); /* eslint-disable-next-line */ }, [JSON.stringify(filter), dataRevision]);

  const onSeed = async () => {
    try {
      const n = await generateSampleData();
      toast({ title: `Inserted ${n} sample events`, variant: "success" });
      reload();
    } catch (e) {
      toast({ title: "Sample data failed", description: errorMessage(e), variant: "destructive" });
    }
  };

  const onPurgeSamples = async () => {
    try {
      const n = await purgeSampleData();
      toast({
        title: n > 0 ? `Removed ${n} sample events` : "No sample data to remove",
        variant: "success",
      });
      reload();
    } catch (e) {
      toast({ title: "Failed to remove sample data", description: errorMessage(e), variant: "destructive" });
    }
  };

  if (loading && !stats) {
    return (
      <div>
        <PageHeader title="Overview" description="Real-time AI token and cost analytics — all local." />
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
          {Array.from({ length: 8 }).map((_, i) => <Skeleton key={i} className="h-28" />)}
        </div>
      </div>
    );
  }

  if (!stats) {
    return (
      <div>
        <PageHeader title="Overview" description="Real-time AI token and cost analytics — all local." />
        <EmptyState
          title="Couldn't load stats"
          description="The local database hasn't responded yet. Try again in a moment."
        />
      </div>
    );
  }

  const unacknowledgedAlerts = alerts.filter((a) => !a.acknowledged_at);
  const empty = stats.sessions_count === 0 && stats.tokens_lifetime === 0;

  const rangeLabel = range === "1d"
    ? "today"
    : range === "7d"
      ? "last 7 days"
      : range === "30d"
        ? "last 30 days"
        : range === "90d"
          ? "last 90 days"
          : "selected period";
  const periodTokensDelta = formatDelta(stats.period_tokens, stats.prev_period_tokens);
  const periodCostDelta = formatDelta(stats.period_cost_usd, stats.prev_period_cost_usd);
  const prevRangeLabel = range === "1d"
    ? "vs yesterday"
    : range === "7d"
      ? "vs prior 7 days"
      : range === "30d"
        ? "vs prior 30 days"
        : range === "90d"
          ? "vs prior 90 days"
          : "";

  return (
    <div className="animate-fade-in">
      <PageHeader
        title="Overview"
        description="All your AI token and cost signals in one place. Nothing leaves this device."
        actions={
          <div className="flex items-center gap-2">
            {empty ? (
              <Button onClick={onSeed}>
                <Sparkles className="h-4 w-4" /> Generate sample data
              </Button>
            ) : null}
            <Button variant="outline" onClick={onPurgeSamples} title="Delete every event from the built-in sample source">
              <X className="h-4 w-4" /> Remove sample data
            </Button>
          </div>
        }
      />

      {unacknowledgedAlerts.length > 0 ? (
        <div className="mb-6 space-y-2">
          {unacknowledgedAlerts.slice(0, 3).map((a) => (
            <div
              key={a.id}
              className={`flex items-start gap-3 p-3 rounded-md border text-sm ${
                a.severity === "critical"
                  ? "border-destructive/50 bg-destructive/5"
                  : "border-amber-500/40 bg-amber-500/5"
              }`}
            >
              <AlertTriangle className={`h-4 w-4 mt-0.5 shrink-0 ${a.severity === "critical" ? "text-destructive" : "text-amber-500"}`} />
              <div className="flex-1 min-w-0">
                <div className="font-medium">{a.title}</div>
                <div className="text-xs text-muted-foreground mt-0.5">{a.message}</div>
              </div>
              <button
                onClick={async () => {
                  try {
                    const { acknowledgeAlert } = await import("@/lib/tauri");
                    await acknowledgeAlert(a.id);
                    setAlerts((al) => al.map((x) => x.id === a.id ? { ...x, acknowledged_at: new Date().toISOString() } : x));
                  } catch {}
                }}
                className="text-muted-foreground hover:text-foreground"
                title="Dismiss"
              >
                <X className="h-3.5 w-3.5" />
              </button>
            </div>
          ))}
        </div>
      ) : null}

      {stats.unpriced_events > 0 ? (
        <div className="mb-4 flex items-start gap-3 p-3 rounded-md border border-amber-500/40 bg-amber-500/5 text-sm">
          <AlertTriangle className="h-4 w-4 mt-0.5 shrink-0 text-amber-500" />
          <div>
            <div className="font-medium">Missing pricing for {formatNumber(stats.unpriced_events)} events</div>
            <div className="text-xs text-muted-foreground mt-0.5">
              {formatNumber(stats.unpriced_tokens)} tokens could not be costed — totals may under-report spend.
              Open Settings → Pricing to add rates, enable estimate mode, then recalculate costs.
            </div>
          </div>
        </div>
      ) : null}

      {empty ? (
        <EmptyState
          title="No data yet"
          description={
            isTauri
              ? "Add a source in Settings, point it at your OpenCode log folder, and click Scan. Or generate sample data to explore the dashboard."
              : "You're running in browser mode with mock data. The Tauri build will connect to your local logs."
          }
          action={
            isTauri ? (
              <Button onClick={onSeed}>
                <Sparkles className="h-4 w-4" /> Generate sample data
              </Button>
            ) : null
          }
        />
      ) : (
        <>
          <div className="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-3 2xl:grid-cols-4 gap-4">
            {range === "all" ? (
              <>
                <TokensCard value={stats!.tokens_lifetime} hint="all time" />
                <CostCard value={stats!.cost_lifetime_usd} hint="USD all time" />
              </>
            ) : (
              <>
                <TokensCard
                  value={stats!.period_tokens}
                  hint={`${rangeLabel} · ${prevRangeLabel}`}
                  delta={periodTokensDelta}
                />
                <CostCard
                  value={stats!.period_cost_usd}
                  hint={`USD ${rangeLabel} · ${prevRangeLabel}`}
                  delta={periodCostDelta}
                />
              </>
            )}
            <TokensCard value={stats!.tokens_today} hint="tokens today" />
            <TokensCard value={stats!.tokens_week} hint="last 7 days" />
            <CostCard value={stats!.cost_today_usd} hint="USD today" />
            <CostCard value={stats!.cost_month_usd} hint="USD last 30 days" />
            <StatCard
              title="Most used model"
              value={stats!.most_used_model ?? "—"}
              hint="by token count"
              icon={Cpu}
            />
            <StatCard
              title="Most expensive model"
              value={stats!.most_expensive_model ?? "—"}
              hint="by cost"
              icon={DollarSign}
            />
            <StatCard
              title="Sessions"
              value={formatNumber(stats!.sessions_count)}
              hint="unique sessions"
              icon={MessageSquare}
            />
            <StatCard
              title="Avg tokens / session"
              value={formatNumber(stats!.avg_tokens_per_session)}
              icon={TrendingUp}
            />
            <StatCard
              title="Reasoning"
              value={formatPercent(stats!.reasoning_token_pct)}
              hint="of all tokens"
            />
            <StatCard
              title="Input : Output"
              value={
                stats!.input_output_ratio > 0
                  ? `${stats!.input_output_ratio.toFixed(2)} : 1`
                  : "—"
              }
            />
            <StatCard
              title="Cache savings"
              value={formatUsd(stats!.cache_savings_usd)}
              hint="vs full input rate"
            />
            <StatCard
              title="Largest session"
              value={formatNumber(stats!.largest_session_tokens)}
              hint="tokens in one session"
              icon={AlertTriangle}
            />
          </div>

          <div className="grid grid-cols-1 xl:grid-cols-12 gap-4 mt-6">
            <Card className="xl:col-span-6">
              <CardHeader>
                <CardTitle className="text-sm font-medium">Token usage over time</CardTitle>
              </CardHeader>
              <CardContent className="min-h-[280px]">
                {series.length > 0 ? <TokensAreaChart data={series} /> : <EmptyState title="No timeseries data" />}
              </CardContent>
            </Card>
            <Card className="xl:col-span-6">
              <CardHeader>
                <CardTitle className="text-sm font-medium">Cost over time</CardTitle>
              </CardHeader>
              <CardContent className="min-h-[280px]">
                {series.length > 0 ? <CostLineChart data={series} /> : <EmptyState title="No cost data" />}
              </CardContent>
            </Card>
            <Card className="xl:col-span-7">
              <CardHeader>
                <CardTitle className="text-sm font-medium">Usage by model</CardTitle>
              </CardHeader>
              <CardContent className="min-h-[280px]">
                {models.length > 0 ? <ModelBarChart data={models} /> : <EmptyState title="No model data" />}
              </CardContent>
            </Card>
            <Card className="xl:col-span-5">
              <CardHeader>
                <CardTitle className="text-sm font-medium">Usage by provider</CardTitle>
              </CardHeader>
              <CardContent className="min-h-[280px]">
                {providers.length > 0 ? <ProviderDonut data={providers} /> : <EmptyState title="No provider data" />}
              </CardContent>
            </Card>
          </div>

          <Card className="mt-6">
            <CardHeader>
              <CardTitle className="text-sm font-medium">Data quality</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-4 text-sm">
                <div>
                  <div className="text-emerald-500 text-2xl font-semibold tabular-nums">
                    {formatNumber(stats!.exactness_mix.exact)}
                  </div>
                  <div className="text-xs text-muted-foreground">Exact events</div>
                </div>
                <div>
                  <div className="text-amber-500 text-2xl font-semibold tabular-nums">
                    {formatNumber(stats!.exactness_mix.estimated)}
                  </div>
                  <div className="text-xs text-muted-foreground">Est. cost only</div>
                </div>
                <div>
                  <div className="text-blue-500 text-2xl font-semibold tabular-nums">
                    {formatNumber(stats!.exactness_mix.mixed)}
                  </div>
                  <div className="text-xs text-muted-foreground">Exact tokens (Cursor)</div>
                </div>
                <div>
                  <div className="text-zinc-500 text-2xl font-semibold tabular-nums">
                    {formatNumber(stats!.exactness_mix.unknown)}
                  </div>
                  <div className="text-xs text-muted-foreground">Unknown events</div>
                </div>
              </div>
            </CardContent>
          </Card>

          <div className="mt-4 text-[10px] text-muted-foreground flex items-center gap-1">
            <Database className="h-3 w-3" />
            All analytics computed locally. No external API calls.
          </div>
        </>
      )}
    </div>
  );
}
