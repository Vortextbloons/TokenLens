import { useEffect, useState } from "react";
import { useParams, Link } from "react-router-dom";
import { getSessionDetail, getSessionEvents } from "@/lib/tauri";
import type { Session, UsageEvent } from "@/types/contracts";
import { PageHeader } from "@/components/layout/PageHeader";
import { Card, CardContent, CardHeader, CardTitle, Skeleton, Badge, Separator } from "@/components/ui/primitives";
import { TokensAreaChart } from "@/charts";
import { formatDate, formatNumber, formatUsd, exactnessColor } from "@/lib/utils";
import { ArrowLeft, Clock, Cpu, DollarSign, Hash } from "lucide-react";

export function SessionDetail() {
  const { id } = useParams<{ id: string }>();
  const sessionId = Number(id);
  const [session, setSession] = useState<Session | null>(null);
  const [events, setEvents] = useState<UsageEvent[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    setLoading(true);
    Promise.all([getSessionDetail(sessionId), getSessionEvents(sessionId)])
      .then(([s, e]) => {
        setSession(s);
        setEvents(e);
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

  const peakContext = events.reduce((max, e) => Math.max(max, e.input_tokens + e.cache_read_tokens), 0);

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
            {session.exactness}
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
            <div className="text-xs text-muted-foreground">Peak context: {formatNumber(peakContext)}</div>
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
