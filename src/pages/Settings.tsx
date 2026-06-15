import { useEffect, useState } from "react";
import {
  getSettings, updateSettings, getSources, addSource, removeSource, scanSource,
  startWatcher, stopWatcher, listWatchers, discoverDefaultSources, dbSizeMb,
  cleanupRawEvents, vacuumDb, resetAllData, exportCsv, exportJson, backupDb,
  listPricing, upsertPricing, deletePricing, importPricingJson, exportPricing,
  listMissingPricing, recalculateCosts, confirmDialog, isTauri,
  cursorStartLogin, cursorDisconnect, cursorGetStatus, cursorSyncNow, cursorConnectWithToken,
} from "@/lib/tauri";
import type { AppSettings, Source, ModelPricing, CursorConnectionStatus } from "@/types/contracts";
import type { MissingPricingRow } from "@/lib/tauri";
import { PageHeader } from "@/components/layout/PageHeader";
import { Card, CardContent, CardDescription, CardHeader, CardTitle, Switch, Button, Input, Label, Badge, Skeleton, Separator, Tabs, TabsList, TabsTrigger, TabsContent, Textarea } from "@/components/ui/primitives";
import { Plus, Trash2, ScanLine, Play, Square, Database, FileDown, FileUp, Wand2, ListChecks, Save, RefreshCw, AlertTriangle, Link2, Unlink } from "lucide-react";
import { toast } from "@/stores/toast";
import { formatUsd, formatNumber } from "@/lib/utils";

export function Settings() {
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [sources, setSources] = useState<Source[]>([]);
  const [watchers, setWatchers] = useState<[number, string][]>([]);
  const [size, setSize] = useState<number | null>(null);
  const [pricing, setPricing] = useState<ModelPricing[]>([]);
  const [newSource, setNewSource] = useState({ name: "", path: "" });
  const [editingPricing, setEditingPricing] = useState<ModelPricing | null>(null);
  const [importOpen, setImportOpen] = useState(false);
  const [importText, setImportText] = useState("");
  const [missing, setMissing] = useState<MissingPricingRow[] | null>(null);
  const [scanningIds, setScanningIds] = useState<Set<number>>(new Set());
  const [heavyOp, setHeavyOp] = useState(false);
  const [cursorStatus, setCursorStatus] = useState<CursorConnectionStatus | null>(null);
  const [cursorSyncing, setCursorSyncing] = useState(false);
  const [cursorTokenOpen, setCursorTokenOpen] = useState(false);
  const [cursorToken, setCursorToken] = useState("");

  const reloadCursor = async () => {
    try {
      const st = await cursorGetStatus();
      setCursorStatus(st);
    } catch {
      setCursorStatus(null);
    }
  };

  const reload = async () => {
    const [s, src, w, sz, pr] = await Promise.all([
      getSettings(), getSources(), listWatchers(), dbSizeMb(), listPricing(),
    ]);
    setSettings(s);
    setSources(src);
    setWatchers(w);
    setSize(sz);
    setPricing(pr);
    await reloadCursor();
  };

  useEffect(() => { reload().catch(() => {}); }, []);

  useEffect(() => {
    if (!isTauri) return;
    let unlistenConnected: (() => void) | undefined;
    let unlistenSync: (() => void) | undefined;
    let unlistenLoginOk: (() => void) | undefined;
    let unlistenLoginErr: (() => void) | undefined;
    (async () => {
      const { listen } = await import("@tauri-apps/api/event");
      unlistenConnected = await listen("cursor-connected", () => { reloadCursor().catch(() => {}); });
      unlistenSync = await listen("cursor-sync-complete", () => { reload().catch(() => {}); });
      unlistenLoginOk = await listen("cursor-login-success", () => {
        toast({ title: "Cursor connected", variant: "success" });
        reload().catch(() => {});
      });
      unlistenLoginErr = await listen<string>("cursor-login-error", (e) => {
        toast({ title: "Cursor sign-in failed", description: e.payload, variant: "destructive" });
      });
    })().catch(() => {});
    return () => {
      unlistenConnected?.();
      unlistenSync?.();
      unlistenLoginOk?.();
      unlistenLoginErr?.();
    };
  }, []);

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
    if (scanningIds.has(id) || heavyOp) return;
    setScanningIds((prev) => new Set(prev).add(id));
    try {
      const r = await scanSource(id);
      const source = sources.find((s) => s.id === id);
      const isDb = source?.path?.endsWith(".db");
      const label = isDb ? "database" : "files";
      let description = `${r.events_inserted} inserted, ${r.events_skipped_duplicate} duplicates (${r.duration_ms}ms).`;
      if (r.events_inserted === 0 && r.events_skipped_duplicate === 0 && !isDb && r.files_scanned > 0) {
        description =
          "No token events found. OpenCode stores usage in opencode.db — use Discover defaults and scan the OpenCode DB source instead.";
      }
      if (r.errors.length > 0) {
        description += ` ${r.errors.length} error(s).`;
      }
      toast({
        title: `Scanned ${r.files_scanned} ${label}`,
        description,
        variant: r.events_inserted > 0 || r.events_skipped_duplicate > 0 ? "success" : "default",
      });
      reload();
    } catch (e: any) {
      toast({ title: "Scan failed", description: String(e), variant: "destructive" });
    } finally {
      setScanningIds((prev) => {
        const next = new Set(prev);
        next.delete(id);
        return next;
      });
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

  const onCursorConnect = async () => {
    try {
      await cursorStartLogin();
      toast({ title: "Cursor sign-in opened", description: "Complete sign-in in the window that appears." });
    } catch (e: any) {
      toast({ title: "Connect failed", description: String(e), variant: "destructive" });
    }
  };

  const onCursorTokenConnect = async () => {
    if (!cursorToken.trim()) return;
    try {
      await cursorConnectWithToken(cursorToken.trim());
      setCursorToken("");
      setCursorTokenOpen(false);
      toast({ title: "Cursor connected", variant: "success" });
      reload();
    } catch (e: any) {
      toast({ title: "Token connect failed", description: String(e), variant: "destructive" });
    }
  };

  const onCursorSync = async () => {
    if (cursorSyncing) return;
    setCursorSyncing(true);
    try {
      const r = await cursorSyncNow();
      toast({
        title: "Cursor sync complete",
        description: `${r.events_inserted} inserted, ${r.events_skipped_duplicate} duplicates (${r.duration_ms}ms)`,
        variant: r.events_inserted > 0 ? "success" : "default",
      });
      reload();
    } catch (e: any) {
      toast({ title: "Cursor sync failed", description: String(e), variant: "destructive" });
    } finally {
      setCursorSyncing(false);
    }
  };

  const onCursorDisconnect = async () => {
    const ok = await confirmDialog("Disconnect your Cursor account from TokenLens?", {
      title: "Disconnect Cursor",
      kind: "warning",
    });
    if (!ok) return;
    try {
      await cursorDisconnect();
      toast({ title: "Cursor disconnected" });
      reload();
    } catch (e: any) {
      toast({ title: "Disconnect failed", description: String(e), variant: "destructive" });
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
    if (heavyOp) return;
    setHeavyOp(true);
    try { await vacuumDb(); toast({ title: "Database vacuumed", variant: "success" }); reload(); }
    catch (e: any) { toast({ title: "Vacuum failed", description: String(e), variant: "destructive" }); }
    finally { setHeavyOp(false); }
  };

  const onReset = async () => {
    if (!(await confirmDialog(
      "Reset TokenLens?\n\n" +
      "This will permanently delete all events, sessions, sources, alerts, " +
      "and settings. Model pricing is kept. This cannot be undone.",
      { title: "Reset all data", kind: "warning" }
    ))) return;
    try {
      const s = await resetAllData();
      const total = s.events + s.sessions + s.daily_usage + s.alerts +
                    s.file_offsets + s.inbox_files + s.projects +
                    s.pricing_history + s.sources + s.settings;
      toast({
        title: total > 0 ? `Reset complete (${total} rows cleared)` : "Nothing to reset",
        description: `${s.events} events, ${s.sessions} sessions, ${s.sources} sources, ${s.settings} settings`,
        variant: "destructive",
      });
      reload();
    } catch (e: any) { toast({ title: "Reset failed", description: String(e), variant: "destructive" }); }
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
    if (!(await confirmDialog(`Delete pricing for ${provider}/${model}?`, { kind: "warning" }))) return;
    try { await deletePricing(provider, model); reload(); }
    catch (e: any) { toast({ title: "Delete failed", description: String(e), variant: "destructive" }); }
  };

  // ----- Pricing research workflow (see docs/pricing-research-preset.md) -----

  const loadMissing = async () => {
    try {
      const rows = await listMissingPricing();
      setMissing(rows);
      if (rows.length === 0) {
        toast({ title: "No missing pricing rows", description: "Every in-use model already has a price." });
      }
    } catch (e: any) {
      toast({ title: "Failed to load missing pricing", description: String(e), variant: "destructive" });
    }
  };

  // Export both already-priced rows AND missing rows as a single JSON envelope.
  // The envelope format is what the AI preset consumes; the AI then returns
  // just the rows it can fill in, which we feed back through importPricingJson.
  const onExportForAI = async () => {
    try {
      const [priced, missingRows] = await Promise.all([
        exportPricing(),
        listMissingPricing(),
      ]);
      const envelope = {
        generated_at: new Date().toISOString(),
        instruction: "See docs/pricing-research-preset.md. Output a JSON array of ModelPricing rows; one per missing pair.",
        already_priced: priced,
        missing: missingRows,
      };
      const json = JSON.stringify(envelope, null, 2);
      // Prefer Tauri save dialog when available; otherwise use a clipboard
      // prompt so the user can paste into Cursor.
      if (isTauri) {
        const { save } = await import("@tauri-apps/plugin-dialog");
        const { writeTextFile } = await import("@tauri-apps/plugin-fs");
        const path = await save({
          title: "Save pricing research input",
          defaultPath: "tokenlens-pricing-research.json",
          filters: [{ name: "JSON", extensions: ["json"] }],
        });
        if (!path) return;
        await writeTextFile(path, json);
        toast({ title: "Saved research input", description: path, variant: "success" });
      } else {
        await navigator.clipboard.writeText(json).catch(() => {});
        const ok = confirm(
          "Pricing research JSON copied to clipboard (mock mode — no save dialog).\n\n" +
          "Paste it into the AI preset. The output JSON from the AI goes into Settings → Pricing → Import JSON."
        );
        if (ok) {
          setImportText("");
          setImportOpen(true);
        }
      }
    } catch (e: any) {
      toast({ title: "Export failed", description: String(e), variant: "destructive" });
    }
  };

  // Parse the user-pasted JSON. Accept either a bare array of ModelPricing
  // rows OR the same envelope we produced on export (in which case we look
  // for the array under common keys). We don't try to be too clever — the
  // AI is told to return a bare array.
  const onImport = async () => {
    const raw = importText.trim();
    if (!raw) {
      toast({ title: "Paste JSON first", variant: "destructive" });
      return;
    }
    let parsed: unknown;
    try {
      parsed = JSON.parse(raw);
    } catch (e: any) {
      toast({ title: "Invalid JSON", description: String(e), variant: "destructive" });
      return;
    }
    // Normalize to an array of ModelPricing-shaped objects.
    const arr: unknown[] = Array.isArray(parsed)
      ? (parsed as unknown[])
      : Array.isArray((parsed as any)?.rows)
        ? (parsed as any).rows
        : Array.isArray((parsed as any)?.pricing)
          ? (parsed as any).pricing
          : [];
    if (arr.length === 0) {
      toast({
        title: "No rows found",
        description: "Expected a JSON array of pricing rows (see docs/pricing-research-preset.md).",
        variant: "destructive",
      });
      return;
    }
    const rows: ModelPricing[] = arr.map((r: any, i: number) => ({
      id: null,
      provider: String(r.provider ?? "").trim(),
      model: String(r.model ?? "").trim(),
      input_price_per_million: Number(r.input_price_per_million ?? 0),
      output_price_per_million: Number(r.output_price_per_million ?? 0),
      reasoning_price_per_million: Number(r.reasoning_price_per_million ?? 0),
      cache_read_price_per_million: Number(r.cache_read_price_per_million ?? 0),
      cache_write_price_per_million: Number(r.cache_write_price_per_million ?? 0),
      currency: String(r.currency ?? "USD"),
      effective_date: r.effective_date ?? null,
      is_local: Boolean(r.is_local),
      source: String(r.source ?? "manual"),
      updated_at: "",
      // Surface but don't fail on unknown fields so a stray `notes` doesn't
      // break the import.
      ...(typeof r.notes === "string" ? { notes: r.notes } as any : {}),
    }));
    try {
      const summary = await importPricingJson(rows);
      const desc = `received ${summary.received} · inserted ${summary.inserted} · updated ${summary.updated} · skipped ${summary.skipped}` +
        (summary.errors.length ? ` · ${summary.errors.length} error(s)` : "");
      toast({
        title: "Pricing imported",
        description: desc,
        variant: summary.errors.length ? "default" : "success",
      });
      setImportOpen(false);
      setImportText("");
      // Refresh pricing list, then ask the user whether to recalc.
      await reload();
      if (await confirmDialog(
        "Recalculate costs now?\n\n" +
        "This walks every usage_event and recomputes cost_usd from the new pricing table. " +
        "Recommended after a bulk import.",
        { title: "Recalculate costs", kind: "info" }
      )) {
        setHeavyOp(true);
        try {
          const n = await recalculateCosts();
          toast({ title: `Recalculated ${n} events`, variant: "success" });
        } finally {
          setHeavyOp(false);
        }
      }
    } catch (e: any) {
      toast({ title: "Import failed", description: String(e), variant: "destructive" });
    }
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
          <Card className="mb-3">
            <CardHeader>
              <CardTitle className="text-sm">Cursor</CardTitle>
              <CardDescription>
                Sign in to pull usage events from your Cursor account. Token counts are exact;
                included-plan usage shows API-equivalent cost estimates.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-3">
              {cursorStatus?.connected ? (
                <div className="flex flex-col sm:flex-row sm:items-center gap-3 p-3 rounded-md border bg-card">
                  <div className="flex-1 min-w-0 space-y-1">
                    <div className="font-medium text-sm">{cursorStatus.email_or_user_label ?? "Cursor account"}</div>
                    <div className="flex flex-wrap gap-2">
                      <Badge variant="success">connected</Badge>
                      {cursorStatus.expires_at ? (
                        <span className="text-[10px] text-muted-foreground">expires {cursorStatus.expires_at}</span>
                      ) : null}
                      {cursorStatus.last_sync_at ? (
                        <span className="text-[10px] text-muted-foreground">last sync {cursorStatus.last_sync_at}</span>
                      ) : null}
                    </div>
                    {cursorStatus.last_sync_result ? (
                      <div className="text-[10px] text-muted-foreground">{cursorStatus.last_sync_result}</div>
                    ) : null}
                    <div className="text-[10px] text-muted-foreground">{formatNumber(cursorStatus.events_total)} events imported</div>
                  </div>
                  <Button variant="outline" size="sm" disabled={cursorSyncing} onClick={onCursorSync}>
                    {cursorSyncing ? <RefreshCw className="h-3.5 w-3.5 animate-spin" /> : <RefreshCw className="h-3.5 w-3.5" />}
                    {cursorSyncing ? " Syncing…" : " Sync now"}
                  </Button>
                  <Button variant="ghost" size="sm" onClick={onCursorDisconnect}>
                    <Unlink className="h-3.5 w-3.5 text-destructive" />
                  </Button>
                </div>
              ) : (
                <div className="space-y-2">
                  <Button onClick={onCursorConnect}>
                    <Link2 className="h-3.5 w-3.5" /> Connect Cursor
                  </Button>
                  <Button variant="ghost" size="sm" onClick={() => setCursorTokenOpen((v) => !v)}>
                    Advanced: paste session token
                  </Button>
                  {cursorTokenOpen ? (
                    <div className="space-y-2 pt-1">
                      <Textarea
                        placeholder="WorkosCursorSessionToken from browser DevTools → Application → Cookies"
                        value={cursorToken}
                        onChange={(e) => setCursorToken(e.target.value)}
                        className="min-h-[60px] text-xs"
                      />
                      <Button size="sm" variant="outline" onClick={onCursorTokenConnect} disabled={!cursorToken.trim()}>
                        Connect with token
                      </Button>
                    </div>
                  ) : null}
                </div>
              )}
            </CardContent>
          </Card>

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
                      <Button
                        variant="outline"
                        size="sm"
                        disabled={scanningIds.has(s.id) || heavyOp}
                        onClick={() => onScan(s.id)}
                      >
                        {scanningIds.has(s.id) ? (
                          <RefreshCw className="h-3.5 w-3.5 animate-spin" />
                        ) : (
                          <ScanLine className="h-3.5 w-3.5" />
                        )}
                        {scanningIds.has(s.id) ? " Scanning…" : " Scan"}
                      </Button>
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
              <CardTitle className="text-sm">Pricing research workflow</CardTitle>
              <CardDescription>
                Use an external AI session to look up missing rates. Export the model list,
                hand it to the preset, then import the JSON it returns. See{" "}
                <code className="font-mono text-[11px]">docs/pricing-research-preset.md</code>.
              </CardDescription>
            </CardHeader>
            <CardContent className="space-y-3">
              <div className="flex flex-wrap gap-2">
                <Button variant="outline" size="sm" onClick={loadMissing}>
                  <ListChecks className="h-3.5 w-3.5" /> Show missing pricing
                </Button>
                <Button variant="outline" size="sm" onClick={onExportForAI}>
                  <Wand2 className="h-3.5 w-3.5" /> Export for AI preset
                </Button>
                <Button variant="outline" size="sm" onClick={() => { setImportText(""); setImportOpen(true); }}>
                  <FileUp className="h-3.5 w-3.5" /> Import JSON
                </Button>
              </div>
              {missing !== null ? (
                missing.length === 0 ? (
                  <div className="text-xs text-muted-foreground">
                    All in-use models have pricing rows. Use{" "}
                    <em>Show missing pricing</em> again later after new models show up.
                  </div>
                ) : (
                  <div className="rounded-md border">
                    <table className="w-full text-sm">
                      <thead>
                        <tr className="border-b text-muted-foreground text-xs">
                          <th className="text-left p-2">Provider</th>
                          <th className="text-left p-2">Model</th>
                          <th className="text-right p-2">Events</th>
                          <th className="text-right p-2">Tokens</th>
                          <th className="text-right p-2">Cost (currently $0)</th>
                        </tr>
                      </thead>
                      <tbody>
                        {missing.map((m) => (
                          <tr key={`${m.provider}-${m.model}`} className="border-b last:border-0 hover:bg-muted/30">
                            <td className="p-2 font-medium">{m.provider}</td>
                            <td className="p-2 font-mono text-xs">{m.model}</td>
                            <td className="p-2 text-right tabular-nums">{formatNumber(m.events)}</td>
                            <td className="p-2 text-right tabular-nums">{formatNumber(m.total_tokens)}</td>
                            <td className="p-2 text-right tabular-nums text-muted-foreground">{formatUsd(m.current_cost_usd)}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )
              ) : null}
            </CardContent>
          </Card>

          <Card className="mt-4">
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

          {importOpen ? (
            <Card className="mt-4">
              <CardHeader>
                <CardTitle className="text-sm">Import pricing JSON</CardTitle>
                <CardDescription>
                  Paste the JSON array the AI preset produced. Each object should match the
                  ModelPricing schema (provider, model, *_price_per_million, source).
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-3">
                <Textarea
                  rows={10}
                  placeholder={`[\n  {\n    "provider": "openai",\n    "model": "gpt-5.4-mini",\n    "input_price_per_million": 0.25,\n    "output_price_per_million": 2.0,\n    "source": "ai-research:https://openai.com/api/pricing"\n  }\n]`}
                  value={importText}
                  onChange={(e) => setImportText(e.target.value)}
                />
                <div className="flex gap-2">
                  <Button onClick={onImport}><FileUp className="h-3.5 w-3.5" /> Import</Button>
                  <Button variant="ghost" onClick={() => { setImportOpen(false); setImportText(""); }}>Cancel</Button>
                </div>
                <div className="text-xs text-muted-foreground">
                  Tip: import is idempotent. Re-importing the same JSON updates existing rows
                  and records an entry in <code className="font-mono">pricing_history</code>.
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
