import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { useFilterObject } from "@/stores/filter";
import { useDataRevision } from "@/stores/dataRevision";
import { getSessions } from "@/lib/tauri";
import type { Session } from "@/types/contracts";
import { PageHeader } from "@/components/layout/PageHeader";
import { Card, CardContent } from "@/components/ui/primitives";
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow, Badge, Skeleton, Input } from "@/components/ui/primitives";
import { formatDate, formatNumber, formatUsd, exactnessColor, formatExactness } from "@/lib/utils";
import { Search, ChevronRight } from "lucide-react";

export function Sessions() {
  const filter = useFilterObject();
  const dataRevision = useDataRevision((s) => s.revision);
  const [sessions, setSessions] = useState<Session[] | null>(null);
  const [query, setQuery] = useState("");

  useEffect(() => {
    setSessions(null);
    getSessions({ ...filter, limit: 500 }).then(setSessions).catch(() => setSessions([]));
    // eslint-disable-next-line
  }, [JSON.stringify(filter), dataRevision]);

  const filtered = (sessions ?? []).filter((s) => {
    if (!query) return true;
    const q = query.toLowerCase();
    return (
      s.title?.toLowerCase().includes(q) ||
      s.source_session_id.toLowerCase().includes(q) ||
      s.model?.toLowerCase().includes(q) ||
      s.provider?.toLowerCase().includes(q)
    );
  });

  return (
    <div className="animate-fade-in">
      <PageHeader title="Sessions" description="Per-session token and cost breakdown." />
      <div className="mb-4 flex items-center gap-2 max-w-md">
        <div className="relative flex-1">
          <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground" />
          <Input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search by project, model, provider…"
            className="pl-8"
          />
        </div>
      </div>
      <Card>
        <CardContent className="p-0">
          {sessions === null ? (
            <div className="p-6 space-y-2">
              {Array.from({ length: 6 }).map((_, i) => <Skeleton key={i} className="h-12" />)}
            </div>
          ) : filtered.length === 0 ? (
            <div className="py-16 text-center text-sm text-muted-foreground">No sessions match this filter.</div>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Session</TableHead>
                  <TableHead>Provider</TableHead>
                  <TableHead>Model</TableHead>
                  <TableHead className="text-right">Tokens</TableHead>
                  <TableHead className="text-right">Cost</TableHead>
                  <TableHead>Last seen</TableHead>
                  <TableHead>Exactness</TableHead>
                  <TableHead></TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {filtered.map((s) => (
                  <TableRow key={s.id}>
                    <TableCell>
                      <div className="font-medium truncate max-w-[260px]">{s.title ?? s.source_session_id}</div>
                      <div className="text-[10px] text-muted-foreground font-mono">{s.source_session_id}</div>
                    </TableCell>
                    <TableCell>
                      {s.provider ? <Badge variant="secondary">{s.provider}</Badge> : <span className="text-muted-foreground">—</span>}
                    </TableCell>
                    <TableCell className="text-sm">{s.model ?? "—"}</TableCell>
                    <TableCell className="text-right tabular-nums">{formatNumber(s.total_tokens)}</TableCell>
                    <TableCell className="text-right tabular-nums">{formatUsd(s.total_cost_usd)}</TableCell>
                    <TableCell className="text-xs text-muted-foreground">{formatDate(s.last_seen_at)}</TableCell>
                    <TableCell>
                      <span className={`text-xs ${exactnessColor(s.exactness)}`}>{formatExactness(s.exactness)}</span>
                    </TableCell>
                    <TableCell>
                      <Link to={`/sessions/${s.id}`} className="text-muted-foreground hover:text-foreground inline-flex">
                        <ChevronRight className="h-4 w-4" />
                      </Link>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
