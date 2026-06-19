import { useEffect, useState } from "react";
import { useParams, Link } from "react-router-dom";
import { getSessionDetail, getSessionEvents, listPricing, simulateSessionSwap } from "@/lib/tauri";
import type { ModelPricing, Session, SessionSwapQuote, UsageEvent } from "@/types/contracts";
import { PageHeader } from "@/components/layout/PageHeader";
import { Card, CardContent, CardHeader, CardTitle, Skeleton, Badge } from "@/components/ui/primitives";
import { TokensAreaChart } from "@/charts";
import { formatDate, formatNumber, formatUsd, exactnessColor, formatExactness, formatPercent } from "@/lib/utils";
import { peakContextForEvents, resolveContextWindow } from "@/lib/cost";
import { ArrowLeft, Clock, Cpu, DollarSign, Hash, Wand2 } from "lucide-react";

export function SessionDetail() {
  const { id } = useParams<{ id: string }>();
  const sessionId = Number(id);
  const [session, setSession] = useState<Session | null>(null);
  const [events, setEvents] = useState<UsageEvent[]>([]);
  const [pricing, setPricing] = useState<ModelPricing[]>([]);
  const [swapProvider, setSwapProvider] = useState("");
  const [swapModel, setSwapModel] = useState("");
  const [swapQuote, setSwapQuote] = useState<SessionSwapQuote | null>(null);
  const [swapLoading, setSwapLoading] = useState(false);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading(true);
    Promise.all([getSessionDetail(sessionId), getSessionEvents(sessionId), listPricing()])
      .then(([s, e, p]) => {
        setSession(s);
        setEvents(e);
        setPricing(p);
        const defaultProvider = s?.provider ?? e[0]?.provider ?? p[0]?.provider ?? "";
        const defaultModel = s?.model ?? e[0]?.model ?? p[0]?.model ?? "";
        setSwapProvider(defaultProvider);
        setSwapModel(defaultModel);
        setSwapQuote(null);
      })
      .finally(() => setLoading(false));
  }, [sessionId]);

  if (loading) {
    return <div className="space-y-4">
      <Skeleton className="h-10 w-1/3" />
      <Skeleton className="h-32" />
    </div>;
  }

  if (!session) {
    return (
      <div>
        <PageHeader title="Session not found" />
        <Link to="/sessions" className="text-sm text-primary">← Back to sessions</Link>
      </div>
    );
  }

  const peakContext = peakContextForEvents(events);
  const currentLimit = resolveContextWindow(session.provider ?? "", session.model ?? "", pricing);
  const currentUtilization = currentLimit ? (peakContext / currentLimit) * 100 : 0;

  const onSimulateSwap = async () => {
    if (!swapProvider.trim() || !swapModel.trim()) return;
    setSwapLoading(true);
    try {
      setSwapQuote(await simulateSessionSwap(sessionId, swapProvider.trim(), swapModel.trim()));
    } finally {
      setSwapLoading(false);
    }
  };

  // Build a small timeseries for the session
  const seriesMap: Record<string, any> = {};
  for (const e of events) {
    const d = e.timestamp.slice(0, 10);
    seriesMap[d] = seriesMap[d] ?? { date: d, input_tokens: 0, output_tokens: 0, reasoning_tokens: 0, cache_read_tokens: 0, total_tokens: 0, cost_usd: 0 };
    seriesMap[d].input_tokens += e.input_tokens;
    seriesMap[d].output_tokens += e.output_tokens;
    seriesMap[d].reasoning_tokens += e.reasoning_tokens;
    seriesMap[d].cache_read_tokens += e.cache_read_tokens;
    seriesMap[d].total_tokens += e.total_tokens;
    seriesMap[d].cost_usd += e.cost_usd;
  }
  const series = Object.values(seriesMap);

  return (
    <div className="animate-fade-in">
      <Link to="/sessions" className="text-sm text-muted-foreground hover:text-foreground inline-flex items-center gap-1 mb-2">
        <ArrowLeft className="h-3.5 w-3.5" /> Back to sessions
      </Link>
      <PageHeader
        title={session.title ?? session.source_session_id}
        description={`Source session ${session.source_session_id}`}
        actions={
          <Badge variant="outline" className={exactnessColor(session.exactness)}>
            {formatExactness(session.exactness)}
          </Badge>
        }
      />
        <div className="grid grid-cols-2 md:grid-cols-4 gap-4 mb-6">
        <Card>
          <CardContent className="pt-5">
            <div className="text-xs text-muted-foreground flex items-center gap-1 mb-1"><Hash className="h-3 w-3" /> Total tokens</div>
            <div className="text-xl font-semibold tabular-nums">{formatNumber(session.total_tokens)}</div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-5">
            <div className="text-xs text-muted-foreground flex items-center gap-1 mb-1"><DollarSign className="h-3 w-3" /> Total cost</div>
            <div className="text-xl font-semibold tabular-nums text-violet-500">{formatUsd(session.total_cost_usd)}</div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-5">
            <div className="text-xs text-muted-foreground flex items-center gap-1 mb-1"><Cpu className="h-3 w-3" /> Model</div>
            <div className="text-sm font-medium">{session.model ?? "—"}</div>
            <div className="text-xs text-muted-foreground">{session.provider ?? "—"}</div>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-5">
            <div className="text-xs text-muted-foreground flex items-center gap-1 mb-1"><Clock className="h-3 w-3" /> Last activity</div>
            <div className="text-sm font-medium">{formatDate(session.last_seen_at)}</div>
            <div className="text-xs text-muted-foreground">
              Peak context: {formatNumber(peakContext)}{currentLimit ? ` · ${formatPercent(currentUtilization)} of limit` : ""}
            </div>
          </CardContent>
        </Card>
      </div>

      <div className="grid grid-cols-1 xl:grid-cols-2 gap-4 mb-6">
        <Card>
          <CardHeader><CardTitle className="text-sm font-medium">Model swap simulator</CardTitle></CardHeader>
          <CardContent className="space-y-3">
            <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
              <div>
                <div className="text-xs text-muted-foreground mb-1">Target provider</div>
                <input
                  value={swapProvider}
                  onChange={(e) => setSwapProvider(e.target.value)}
                  className="h-9 w-full rounded-md border bg-background px-3 text-sm"
                  placeholder="openai"
                />
              </div>
              <div>
                <div className="text-xs text-muted-foreground mb-1">Target model</div>
                <input
                  value={swapModel}
                  onChange={(e) => setSwapModel(e.target.value)}
                  className="h-9 w-full rounded-md border bg-background px-3 text-sm"
                  placeholder="gpt-5.4-mini"
                />
              </div>
            </div>
            <div className="flex items-center gap-2">
              <button
                onClick={onSimulateSwap}
                disabled={swapLoading}
                className="inline-flex h-9 items-center gap-2 rounded-md border px-3 text-sm hover:bg-muted disabled:opacity-50"
              >
                <Wand2 className="h-3.5 w-3.5" />
                {swapLoading ? "Simulating…" : "Simulate swap"}
              </button>
              <div className="text-xs text-muted-foreground">
                Recalculates from stored token counts only.
              </div>
            </div>
            {swapQuote ? (
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-3 text-sm">
                <div>
                  <div className="text-xs text-muted-foreground">Current</div>
                  <div className="font-medium tabular-nums">{formatUsd(swapQuote.current_cost_usd)}</div>
                </div>
                <div>
                  <div className="text-xs text-muted-foreground">Simulated</div>
                  <div className="font-medium tabular-nums">{formatUsd(swapQuote.simulated_cost_usd)}</div>
                </div>
                <div>
                  <div className="text-xs text-muted-foreground">Delta</div>
                  <div className={`font-medium tabular-nums ${swapQuote.delta_usd <= 0 ? "text-emerald-500" : "text-rose-500"}`}>
                    {swapQuote.delta_usd <= 0 ? "-" : "+"}{formatUsd(Math.abs(swapQuote.delta_usd))}
                  </div>
                </div>
                <div>
                  <div className="text-xs text-muted-foreground">Status</div>
                  <div className="font-medium capitalize">{swapQuote.target_pricing_status}</div>
                </div>
              </div>
            ) : null}
          </CardContent>
        </Card>

        <Card>
          <CardHeader><CardTitle className="text-sm font-medium">Context utilization</CardTitle></CardHeader>
          <CardContent className="space-y-3">
            <div className="grid grid-cols-2 gap-3 text-sm">
              <div>
                <div className="text-xs text-muted-foreground">Peak context</div>
                <div className="font-medium tabular-nums">{formatNumber(peakContext)}</div>
              </div>
              <div>
                <div className="text-xs text-muted-foreground">Limit</div>
                <div className="font-medium tabular-nums">{currentLimit ? formatNumber(currentLimit) : "—"}</div>
              </div>
              <div>
                <div className="text-xs text-muted-foreground">Utilization</div>
                <div className="font-medium tabular-nums">{currentLimit ? formatPercent(currentUtilization) : "—"}</div>
              </div>
              <div>
                <div className="text-xs text-muted-foreground">Events</div>
                <div className="font-medium tabular-nums">{formatNumber(events.length)}</div>
              </div>
            </div>
            <div className="text-xs text-muted-foreground">
              {currentLimit && currentUtilization >= 80
                ? `You're routinely hitting ${formatPercent(currentUtilization)} on ${session.model ?? "this model"}.`
                : "Context usage is comfortably below the model limit."}
            </div>
          </CardContent>
        </Card>
      </div>

      <Card className="mb-6">
        <CardHeader><CardTitle className="text-sm font-medium">Token usage over time</CardTitle></CardHeader>
        <CardContent>
          {series.length > 0 ? <TokensAreaChart data={series as any} /> : <div className="text-sm text-muted-foreground py-8 text-center">No events</div>}
        </CardContent>
      </Card>

      <Card>
        <CardHeader><CardTitle className="text-sm font-medium">Events ({events.length})</CardTitle></CardHeader>
        <CardContent className="p-0">
          <div className="divide-y">
            {events.map((e, i) => (
              <div key={e.event_hash + i} className="px-5 py-3 hover:bg-muted/30 flex items-center gap-3 text-sm">
                <div className="text-xs text-muted-foreground w-32 shrink-0">{formatDate(e.timestamp)}</div>
                <Badge variant="secondary" className="w-20 justify-center">{e.event_type}</Badge>
                <div className="flex-1 min-w-0">
                  <div className="font-medium truncate">{e.model ?? "—"}</div>
                  <div className="text-[10px] text-muted-foreground">
                    in {formatNumber(e.input_tokens)} · out {formatNumber(e.output_tokens)}
                    {e.reasoning_tokens > 0 ? ` · reasoning ${formatNumber(e.reasoning_tokens)}` : ""}
                    {e.cache_read_tokens > 0 ? ` · cache ${formatNumber(e.cache_read_tokens)}` : ""}
                  </div>
                </div>
                <div className="text-right shrink-0">
                  <div className="tabular-nums">{formatNumber(e.total_tokens)}</div>
                  <div className="text-[10px] text-muted-foreground tabular-nums">{formatUsd(e.cost_usd)}</div>
                </div>
              </div>
            ))}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
