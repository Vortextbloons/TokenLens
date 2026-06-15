import { useEffect, useState } from "react";
import { useFilterObject } from "@/stores/filter";
import { listEvents, listPricing } from "@/lib/tauri";
import type { UsageEvent, ModelPricing } from "@/types/contracts";
import { PageHeader } from "@/components/layout/PageHeader";
import { Card, CardContent, CardHeader, CardTitle, Skeleton, Badge, Input, Select } from "@/components/ui/primitives";
import { Search, Copy, Check } from "lucide-react";
import { formatDate, formatNumber, formatUsd, exactnessColor } from "@/lib/utils";
import { EmptyState } from "@/components/layout/PageHeader";

export function RawEvents() {
  const filter = useFilterObject();
  const [events, setEvents] = useState<UsageEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [query, setQuery] = useState("");
  const [exactness, setExactness] = useState("any");
  const [selected, setSelected] = useState<UsageEvent | null>(null);
  const [pricing, setPricing] = useState<ModelPricing[]>([]);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    setLoading(true);
    listEvents({ ...filter, exactness: exactness === "any" ? null : exactness, limit: 1000 })
      .then(setEvents)
      .finally(() => setLoading(false));
    // eslint-disable-next-line
  }, [JSON.stringify(filter), exactness]);

  useEffect(() => { listPricing().then(setPricing).catch(() => {}); }, []);

  const filtered = events.filter((e) => {
    if (!query) return true;
    const q = query.toLowerCase();
    return (
      e.event_type.toLowerCase().includes(q) ||
      (e.model ?? "").toLowerCase().includes(q) ||
      (e.provider ?? "").toLowerCase().includes(q) ||
      (e.message_role ?? "").toLowerCase().includes(q) ||
      e.event_hash.toLowerCase().includes(q)
    );
  });

  const onCopy = (text: string) => {
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 1200);
  };

  return (
    <div className="animate-fade-in">
      <PageHeader title="Raw Events" description="Inspect individual events. Toggle raw JSON in Settings to control storage." />
      <div className="flex items-center gap-2 mb-4">
        <div className="relative flex-1 max-w-md">
          <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-muted-foreground" />
          <Input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Search hash, model, provider, role…" className="pl-8" />
        </div>
        <Select
          value={exactness}
          onValueChange={setExactness}
          className="w-36"
          options={[
            { value: "any", label: "Any exactness" },
            { value: "exact", label: "Exact only" },
            { value: "estimated", label: "Estimated" },
            { value: "mixed", label: "Mixed" },
            { value: "unknown", label: "Unknown" },
          ]}
        />
      </div>
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4">
        <Card className="lg:col-span-2">
          <CardContent className="p-0">
            {loading ? (
              <div className="p-5 space-y-2">{Array.from({ length: 8 }).map((_, i) => <Skeleton key={i} className="h-10" />)}</div>
            ) : filtered.length === 0 ? (
              <EmptyState title="No events" description="Try a wider date range or relax the exactness filter." />
            ) : (
              <div className="divide-y max-h-[640px] overflow-y-auto scrollbar-thin">
                {filtered.map((e) => (
                  <button
                    key={e.event_hash}
                    onClick={() => setSelected(e)}
                    className={`w-full text-left px-4 py-2.5 hover:bg-muted/40 text-xs flex items-center gap-3 ${selected?.event_hash === e.event_hash ? "bg-muted/60" : ""}`}
                  >
                    <span className="text-muted-foreground w-28 shrink-0">{formatDate(e.timestamp)}</span>
                    <Badge variant="secondary" className="w-16 justify-center">{e.event_type}</Badge>
                    <span className="font-medium flex-1 truncate">{e.model ?? e.provider ?? "(unknown)"}</span>
                    <span className="text-muted-foreground">{e.message_role ?? ""}</span>
                    <span className="tabular-nums w-20 text-right">{formatNumber(e.total_tokens)}</span>
                    <span className={`tabular-nums w-16 text-right ${exactnessColor(e.exactness)}`}>{e.exactness[0].toUpperCase()}</span>
                  </button>
                ))}
              </div>
            )}
          </CardContent>
        </Card>
        <Card>
          <CardHeader>
            <CardTitle className="text-sm font-medium">Event detail</CardTitle>
          </CardHeader>
          <CardContent>
            {!selected ? (
              <div className="text-sm text-muted-foreground py-12 text-center">Select an event to inspect</div>
            ) : (
              <div className="space-y-3 text-sm">
                <div>
                  <div className="text-[10px] text-muted-foreground uppercase">Hash</div>
                  <div className="font-mono text-[10px] break-all">{selected.event_hash}</div>
                </div>
                <div className="grid grid-cols-2 gap-2 text-xs">
                  <div><div className="text-muted-foreground">Time</div><div>{formatDate(selected.timestamp)}</div></div>
                  <div><div className="text-muted-foreground">Type</div><div>{selected.event_type}</div></div>
                  <div><div className="text-muted-foreground">Provider</div><div>{selected.provider ?? "—"}</div></div>
                  <div><div className="text-muted-foreground">Model</div><div className="truncate">{selected.model ?? "—"}</div></div>
                  <div><div className="text-muted-foreground">Role</div><div>{selected.message_role ?? "—"}</div></div>
                  <div><div className="text-muted-foreground">Exactness</div><div className={exactnessColor(selected.exactness)}>{selected.exactness}</div></div>
                </div>
                <div className="grid grid-cols-2 gap-2 text-xs">
                  <div><div className="text-muted-foreground">Input</div><div className="tabular-nums">{formatNumber(selected.input_tokens)}</div></div>
                  <div><div className="text-muted-foreground">Output</div><div className="tabular-nums">{formatNumber(selected.output_tokens)}</div></div>
                  <div><div className="text-muted-foreground">Reasoning</div><div className="tabular-nums">{formatNumber(selected.reasoning_tokens)}</div></div>
                  <div><div className="text-muted-foreground">Cache read</div><div className="tabular-nums">{formatNumber(selected.cache_read_tokens)}</div></div>
                  <div className="col-span-2"><div className="text-muted-foreground">Cost</div><div className="tabular-nums">{formatUsd(selected.cost_usd)}</div></div>
                </div>
                {selected.raw_json ? (
                  <div>
                    <div className="flex items-center justify-between mb-1">
                      <div className="text-[10px] text-muted-foreground uppercase">Raw JSON</div>
                      <button onClick={() => onCopy(selected.raw_json!)} className="text-[10px] text-muted-foreground hover:text-foreground inline-flex items-center gap-1">
                        {copied ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />} {copied ? "Copied" : "Copy"}
                      </button>
                    </div>
                    <pre className="text-[10px] font-mono bg-muted p-2 rounded-md max-h-48 overflow-auto scrollbar-thin whitespace-pre-wrap break-all">
                      {(() => { try { return JSON.stringify(JSON.parse(selected.raw_json), null, 2); } catch { return selected.raw_json; } })()}
                    </pre>
                  </div>
                ) : (
                  <div className="text-[10px] text-muted-foreground italic">Raw JSON not stored. Enable in Settings → Privacy.</div>
                )}
                {selected.raw_source_path ? (
                  <div>
                    <div className="text-[10px] text-muted-foreground uppercase">Source</div>
                    <div className="font-mono text-[10px] truncate">{selected.raw_source_path}</div>
                  </div>
                ) : null}
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}
