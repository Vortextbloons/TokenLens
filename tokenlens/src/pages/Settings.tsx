import { useEffect, useState } from "react";
import { getSettings, updateSettings, getSources, addSource, removeSource, scanSource, startWatcher, stopWatcher, listWatchers, discoverDefaultSources, dbSizeMb, cleanupRawEvents, vacuumDb, resetAllData, exportCsv, exportJson, backupDb, listPricing, upsertPricing, deletePricing } from "@/lib/tauri";
import type { AppSettings, Source, ModelPricing } from "@/types/contracts";
import { PageHeader } from "@/components/layout/PageHeader";
import { Card, CardContent, CardDescription, CardHeader, CardTitle, Switch, Button, Input, Label, Badge, Skeleton, Separator, Tabs, TabsList, TabsTrigger, TabsContent } from "@/components/ui/primitives";
import { Plus, Trash2, ScanLine, Play, Square, Database, FileDown, Save, RefreshCw, AlertTriangle } from "lucide-react";
import { toast } from "@/stores/toast";
import { formatUsd } from "@/lib/utils";

export function Settings() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [sources, setSources] = useState<Source[]>([]);
  const [watchers, setWatchers] = useState<[number, string][]>([]);
  const [size, setSize] = useState<number | null>(null);
  const [pricing, setPricing] = useState<ModelPricing[]>([]);
  const [newSource, setNewSource] = useState({ name: "", path: "" });
  const [editingPricing, setEditingPricing] = useState<ModelPricing | null>(null);

  const reload = async () => {
    const [s, src, w, sz, pr] = await Promise.all([
      getSettings(), getSources(), listWatchers(), dbSizeMb(), listPricing(),
    ]);
    setSettings(s);
    setSources(src);
    setWatchers(w);
    setSize(sz);
    setPricing(pr);
  };

  useEffect(() => { reload().catch(() => {}); }, []);

  const save = async (next: AppSettings) => {
    try {
      const updated = await updateSettings(next);
      setSettings(updated);
      toast({ title: "Settings saved", variant: "success" });
    } catch (e: any) {
      toast({ title: "Save failed", description: String(e), variant: "destructive" });
    }
  };

  const onAddSource = async () => {
    if (!newSource.name || !newSource.path) return;
    try {
      await addSource(newSource.name, "opencode_logs", newSource.path);
      setNewSource({ name: "", path: "" });
      reload();
      toast({ title: "Source added", variant: "success" });
    } catch (e: any) {
      toast({ title: "Add source failed", description: String(e), variant: "destructive" });
    }
  };

  const onScan = async (id: number) => {
    try {
      const r = await scanSource(id);
      toast({
        title: `Scanned ${r.files_scanned} files`,
        description: `${r.events_inserted} inserted, ${r.events_skipped_duplicate} duplicates.`,
        variant: "success",
      });
      reload();
    } catch (e: any) {
      toast({ title: "Scan failed", description: String(e), variant: "destructive" });
    }
  };

  const onRemove = async (id: number) => {
    try { await removeSource(id); reload(); }
    catch (e: any) { toast({ title: "Remove failed", description: String(e), variant: "destructive" }); }
  };

  const onStartWatcher = async (id: number) => {
    try { await startWatcher(id); reload(); toast({ title: "Watcher started", variant: "success" }); }
    catch (e: any) { toast({ title: "Start failed", description: String(e), variant: "destructive" }); }
  };

  const onStopWatcher = async (id: number) => {
    try { await stopWatcher(id); reload(); toast({ title: "Watcher stopped" }); }
    catch (e: any) { toast({ title: "Stop failed", description: String(e), variant: "destructive" }); }
  };

  const onDiscover = async () => {
    try {
      const found = await discoverDefaultSources();
      toast({ title: `Discovered ${found.length} sources`, description: found.map((f) => f.name).join(", ") });
      reload();
    } catch (e: any) {
      toast({ title: "Discovery failed", description: String(e), variant: "destructive" });
    }
  };

  const onCleanup = async () => {
    if (!settings) return;
    try {
      const n = await cleanupRawEvents(settings.raw_retention_days);
      toast({ title: `Cleaned up ${n} old events`, variant: "success" });
      reload();
    } catch (e: any) { toast({ title: "Cleanup failed", description: String(e), variant: "destructive" }); }
  };

  const onVacuum = async () => {
    try { await vacuumDb(); toast({ title: "Database vacuumed", variant: "success" }); reload(); }
    catch (e: any) { toast({ title: "Vacuum failed", description: String(e), variant: "destructive" }); }
  };

  const onReset = async () => {
    if (!confirm("This will delete all events, sessions, and pricing history. Continue?")) return;
    try { await resetAllData(); toast({ title: "All data reset", variant: "destructive" }); reload(); }
    catch (e: any) { toast({ title: "Reset failed", description: String(e), variant: "destructive" }); }
  };

  const onExport = async (kind: "csv" | "json") => {
    const path = prompt(`Enter full output path (e.g. C:\\Users\\you\\Desktop\\tokenlens-export.${kind}):`);
    if (!path) return;
    try {
      const n = kind === "csv" ? await exportCsv({}, path) : await exportJson({}, path);
      toast({ title: `Exported ${n} events`, description: path, variant: "success" });
    } catch (e: any) {
      toast({ title: "Export failed", description: String(e), variant: "destructive" });
    }
  };

  const onBackup = async () => {
    const path = prompt("Enter full output path for the SQLite backup:");
    if (!path) return;
    try { await backupDb(path); toast({ title: "Backup complete", description: path, variant: "success" }); }
    catch (e: any) { toast({ title: "Backup failed", description: String(e), variant: "destructive" }); }
  };

  const onSavePricing = async () => {
    if (!editingPricing) return;
    try {
      await upsertPricing(editingPricing);
      toast({ title: "Pricing saved", variant: "success" });
      setEditingPricing(null);
      reload();
    } catch (e: any) {
      toast({ title: "Save failed", description: String(e), variant: "destructive" });
    }
  };

  const onDeletePricing = async (provider: string, model: string) => {
    if (!confirm(`Delete pricing for ${provider}/${model}?`)) return;
    try { await deletePricing(provider, model); reload(); }
    catch (e: any) { toast({ title: "Delete failed", description: String(e), variant: "destructive" }); }
  };

  if (!settings) {
    return <div className="space-y-4"><Skeleton className="h-12 w-1/3" /><Skeleton className="h-72" /></div>;
  }

  return (
    <div className="animate-fade-in">
      <PageHeader title="Settings" description="Configure sources, pricing, privacy, and storage." />

      <Tabs value="sources" onValueChange={() => {}}>
        <TabsList>
          <TabsTrigger value="sources">Sources</TabsTrigger>
          <TabsTrigger value="pricing">Pricing</TabsTrigger>
          <TabsTrigger value="privacy">Privacy</TabsTrigger>
          <TabsTrigger value="storage">Storage</TabsTrigger>
          <TabsTrigger value="export">Export</TabsTrigger>
        </TabsList>

        {/* SOURCES */}
        <TabsContent value="sources">
          <Card>
            <CardHeader>
              <CardTitle className="text-sm">Data sources</CardTitle>
              <CardDescription>Point TokenLens at your local log folders. Passively reads; never writes to source folders.</CardDescription>
            </CardHeader>
            <CardContent className="space-y-3">
              <Button variant="outline" size="sm" onClick={onDiscover}>
                <ScanLine className="h-3.5 w-3.5" /> Discover default OpenCode paths
              </Button>
              <Separator />
              {sources.length === 0 ? (
                <div className="text-sm text-muted-foreground py-6 text-center">No sources configured.</div>
              ) : (
                <div className="space-y-2">
                  {sources.map((s) => (
                    <div key={s.id} className="flex items-center gap-3 p-3 rounded-md border bg-card">
                      <div className="flex-1 min-w-0">
                        <div className="font-medium text-sm truncate">{s.name}</div>
                        <div className="text-[10px] text-muted-foreground font-mono truncate">{s.path ?? "—"}</div>
                        <div className="flex items-center gap-2 mt-1">
                          <Badge variant="secondary">{s.kind}</Badge>
                          {s.last_scanned_at ? (
                            <span className="text-[10px] text-muted-foreground">scanned {s.last_scanned_at}</span>
                          ) : null}
                          {s.last_error ? (
                            <Badge variant="destructive" className="text-[10px]">error: {s.last_error}</Badge>
                          ) : null}
                          {watchers.find((w) => w[0] === s.id) ? <Badge variant="success">watching</Badge> : null}
                        </div>
                      </div>
                      <Button variant="outline" size="sm" onClick={() => onScan(s.id)}><ScanLine className="h-3.5 w-3.5" /> Scan</Button>
                      {watchers.find((w) => w[0] === s.id) ? (
                        <Button variant="outline" size="sm" onClick={() => onStopWatcher(s.id)}><Square className="h-3.5 w-3.5" /></Button>
                      ) : (
                        <Button variant="outline" size="sm" onClick={() => onStartWatcher(s.id)}><Play className="h-3.5 w-3.5" /></Button>
                      )}
                      <Button variant="ghost" size="sm" onClick={() => onRemove(s.id)}><Trash2 className="h-3.5 w-3.5 text-destructive" /></Button>
                    </div>
                  ))}
                </div>
              )}
              <Separator />
              <div className="space-y-2">
                <Label className="text-xs text-muted-foreground">Add a new source</Label>
                <div className="grid grid-cols-1 sm:grid-cols-[1fr_2fr_auto] gap-2">
                  <Input placeholder="Name" value={newSource.name} onChange={(e) => setNewSource({ ...newSource, name: e.target.value })} />
                  <Input placeholder="C:\Users\you\.local\share\opencode\log" value={newSource.path} onChange={(e) => setNewSource({ ...newSource, path: e.target.value })} />
                  <Button onClick={onAddSource}><Plus className="h-3.5 w-3.5" /> Add</Button>
                </div>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        {/* PRICING */}
        <TabsContent value="pricing">
          <Card>
            <CardHeader>
              <CardTitle className="text-sm">Model pricing</CardTitle>
              <CardDescription>Per-million-token rates used to compute cost. Edit, add custom, or remove.</CardDescription>
            </CardHeader>
            <CardContent className="p-0">
              <table className="w-full text-sm">
                <thead>
                  <tr className="border-b text-muted-foreground text-xs">
                    <th className="text-left p-3">Provider</th>
                    <th className="text-left p-3">Model</th>
                    <th className="text-right p-3">Input / 1M</th>
                    <th className="text-right p-3">Output / 1M</th>
                    <th className="text-right p-3">Reasoning / 1M</th>
                    <th className="text-right p-3">Cache read / 1M</th>
                    <th className="text-right p-3">Cache write / 1M</th>
                    <th></th>
                  </tr>
                </thead>
                <tbody>
                  {pricing.map((p) => (
                    <tr key={`${p.provider}-${p.model}`} className="border-b hover:bg-muted/30">
                      <td className="p-3 font-medium">{p.provider}</td>
                      <td className="p-3">{p.model}</td>
                      <td className="p-3 text-right tabular-nums">{formatUsd(p.input_price_per_million)}</td>
                      <td className="p-3 text-right tabular-nums">{formatUsd(p.output_price_per_million)}</td>
                      <td className="p-3 text-right tabular-nums">{formatUsd(p.reasoning_price_per_million)}</td>
                      <td className="p-3 text-right tabular-nums">{formatUsd(p.cache_read_price_per_million)}</td>
                      <td className="p-3 text-right tabular-nums">{formatUsd(p.cache_write_price_per_million)}</td>
                      <td className="p-3">
                        <div className="flex items-center gap-1">
                          <Button variant="ghost" size="sm" onClick={() => setEditingPricing({ ...p })}>Edit</Button>
                          <Button variant="ghost" size="sm" onClick={() => onDeletePricing(p.provider, p.model)}><Trash2 className="h-3.5 w-3.5 text-destructive" /></Button>
                        </div>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <div className="p-3 border-t">
                <Button onClick={() => setEditingPricing({
                  id: null, provider: "openai", model: "",
                  input_price_per_million: 0, output_price_per_million: 0,
                  reasoning_price_per_million: 0, cache_read_price_per_million: 0,
                  cache_write_price_per_million: 0, currency: "USD",
                  effective_date: null, is_local: false, source: "manual", updated_at: "",
                })}>
                  <Plus className="h-3.5 w-3.5" /> Add pricing row
                </Button>
              </div>
            </CardContent>
          </Card>

          {editingPricing ? (
            <Card className="mt-4">
              <CardHeader>
                <CardTitle className="text-sm">{editingPricing.id ? "Edit" : "New"} pricing</CardTitle>
              </CardHeader>
              <CardContent className="space-y-3">
                <div className="grid grid-cols-2 gap-3">
                  <div>
                    <Label>Provider</Label>
                    <Input value={editingPricing.provider} onChange={(e) => setEditingPricing({ ...editingPricing, provider: e.target.value })} />
                  </div>
                  <div>
                    <Label>Model</Label>
                    <Input value={editingPricing.model} onChange={(e) => setEditingPricing({ ...editingPricing, model: e.target.value })} />
                  </div>
                </div>
                <div className="grid grid-cols-2 sm:grid-cols-3 gap-3">
                  {([
                    ["input_price_per_million", "Input / 1M"],
                    ["output_price_per_million", "Output / 1M"],
                    ["reasoning_price_per_million", "Reasoning / 1M"],
                    ["cache_read_price_per_million", "Cache read / 1M"],
                    ["cache_write_price_per_million", "Cache write / 1M"],
                  ] as const).map(([k, label]) => (
                    <div key={k}>
                      <Label>{label}</Label>
                      <Input
                        type="number" step="0.0001"
                        value={editingPricing[k]}
                        onChange={(e) => setEditingPricing({ ...editingPricing, [k]: Number(e.target.value) } as ModelPricing)}
                      />
                    </div>
                  ))}
                  <div className="flex items-end gap-2">
                    <Switch checked={editingPricing.is_local} onCheckedChange={(v) => setEditingPricing({ ...editingPricing, is_local: v })} id="local" />
                    <Label htmlFor="local">Local model ($0)</Label>
                  </div>
                </div>
                <div className="flex gap-2">
                  <Button onClick={onSavePricing}><Save className="h-3.5 w-3.5" /> Save</Button>
                  <Button variant="ghost" onClick={() => setEditingPricing(null)}>Cancel</Button>
                </div>
              </CardContent>
            </Card>
          ) : null}
        </TabsContent>

        {/* PRIVACY */}
        <TabsContent value="privacy">
          <Card>
            <CardHeader>
              <CardTitle className="text-sm">Privacy</CardTitle>
              <CardDescription>Everything stays on this device. No telemetry, no AI calls, no cloud.</CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="flex items-center justify-between">
                <div>
                  <Label>Store raw JSON for events</Label>
                  <p className="text-xs text-muted-foreground">Required for the Raw Events viewer. Disable to save space.</p>
                </div>
                <Switch checked={settings.store_raw_json} onCheckedChange={(v) => save({ ...settings, store_raw_json: v })} />
              </div>
              <div className="flex items-center justify-between">
                <div>
                  <Label>Store full message text</Label>
                  <p className="text-xs text-muted-foreground">Off by default. TokenLens does not need it for analytics.</p>
                </div>
                <Switch checked={settings.store_message_text} onCheckedChange={(v) => save({ ...settings, store_message_text: v })} />
              </div>
              <div className="flex items-center justify-between">
                <div>
                  <Label>Redact obvious secrets</Label>
                  <p className="text-xs text-muted-foreground">Strips API keys, tokens, private keys from raw JSON before storage.</p>
                </div>
                <Switch checked={settings.redact_secrets} onCheckedChange={(v) => save({ ...settings, redact_secrets: v })} />
              </div>
              <div className="flex items-center justify-between">
                <div>
                  <Label>Anonymize filesystem paths</Label>
                  <p className="text-xs text-muted-foreground">Replace paths in stored data with hashed tokens.</p>
                </div>
                <Switch checked={settings.anonymize_paths} onCheckedChange={(v) => save({ ...settings, anonymize_paths: v })} />
              </div>
              <Separator />
              <div className="flex items-center justify-between">
                <div>
                  <Label>Start on system boot</Label>
                  <p className="text-xs text-muted-foreground">Launch TokenLens when you sign in.</p>
                </div>
                <Switch checked={settings.autostart} onCheckedChange={(v) => save({ ...settings, autostart: v })} />
              </div>
              <div className="flex items-center justify-between">
                <div>
                  <Label>Start minimized to tray</Label>
                  <p className="text-xs text-muted-foreground">Keep collecting in the background.</p>
                </div>
                <Switch checked={settings.start_minimized} onCheckedChange={(v) => save({ ...settings, start_minimized: v })} />
              </div>
              <div className="flex items-center justify-between">
                <div>
                  <Label>Debug logging</Label>
                  <p className="text-xs text-muted-foreground">Writes verbose logs to the app data dir.</p>
                </div>
                <Switch checked={settings.debug_logging} onCheckedChange={(v) => save({ ...settings, debug_logging: v })} />
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        {/* STORAGE */}
        <TabsContent value="storage">
          <Card>
            <CardHeader>
              <CardTitle className="text-sm">Storage</CardTitle>
              <CardDescription>Database size, retention, and cleanup tools.</CardDescription>
            </CardHeader>
            <CardContent className="space-y-4">
              <div className="grid grid-cols-1 sm:grid-cols-3 gap-3">
                <div className="rounded-md border p-3">
                  <div className="text-xs text-muted-foreground">Database size</div>
                  <div className="text-2xl font-semibold tabular-nums">{size !== null ? `${size.toFixed(1)} MB` : "—"}</div>
                </div>
                <div className="rounded-md border p-3">
                  <div className="text-xs text-muted-foreground">Raw JSON retention</div>
                  <div className="flex items-center gap-2">
                    <Input
                      type="number"
                      className="w-20"
                      value={settings.raw_retention_days}
                      onChange={(e) => save({ ...settings, raw_retention_days: Number(e.target.value) })}
                    />
                    <span className="text-xs text-muted-foreground">days</span>
                  </div>
                </div>
                <div className="rounded-md border p-3">
                  <div className="text-xs text-muted-foreground">Active watchers</div>
                  <div className="text-2xl font-semibold tabular-nums">{watchers.length}</div>
                </div>
              </div>
              <div className="flex items-center gap-2">
                <Button variant="outline" onClick={onCleanup}><RefreshCw className="h-3.5 w-3.5" /> Clean old raw events</Button>
                <Button variant="outline" onClick={onVacuum}><Database className="h-3.5 w-3.5" /> Vacuum DB</Button>
                <Button variant="destructive" onClick={onReset}><AlertTriangle className="h-3.5 w-3.5" /> Reset all data</Button>
              </div>
              <div className="text-xs text-muted-foreground">
                Cleanup deletes raw events older than {settings.raw_retention_days} days; aggregate data is preserved.
                Vacuum reclaims disk space. Reset deletes everything except your settings.
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        {/* EXPORT */}
        <TabsContent value="export">
          <Card>
            <CardHeader>
              <CardTitle className="text-sm">Export &amp; backup</CardTitle>
              <CardDescription>CSV, JSON, or a full SQLite backup. Files are written to the path you specify.</CardDescription>
            </CardHeader>
            <CardContent className="space-y-3">
              <div className="flex flex-wrap gap-2">
                <Button variant="outline" onClick={() => onExport("csv")}><FileDown className="h-3.5 w-3.5" /> Export events as CSV</Button>
                <Button variant="outline" onClick={() => onExport("json")}><FileDown className="h-3.5 w-3.5" /> Export events as JSON</Button>
                <Button variant="outline" onClick={onBackup}><Database className="h-3.5 w-3.5" /> Backup database</Button>
              </div>
              <div className="text-xs text-muted-foreground">
                Exports use the current date range filter set in the topbar. The database backup includes the SQLite file and its WAL/SHM sidecars for a consistent snapshot.
              </div>
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
    </div>
  );
}
