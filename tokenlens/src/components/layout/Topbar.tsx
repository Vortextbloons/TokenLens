import { Sun, Moon, Monitor, Sparkles, Database, RefreshCw, Trash2, RotateCcw } from "lucide-react";
import { useTheme } from "@/stores/theme";
import { useFilter } from "@/stores/filter";
import { Button } from "@/components/ui/primitives";
import { Select } from "@/components/ui/primitives";
import { cn } from "@/lib/utils";
import { toast } from "@/stores/toast";
import { generateSampleData, purgeSampleData, resetAllData, isTauri, recalculateCosts, confirmDialog } from "@/lib/tauri";

export function Topbar() {
  const { theme, setTheme } = useTheme();
  const { range, setRange } = useFilter();

  const onGenerateSamples = async () => {
    try {
      const n = await generateSampleData();
      toast({ title: `Generated ${n} sample events`, variant: "success" });
    } catch (e: any) {
      toast({ title: "Failed to generate samples", description: String(e), variant: "destructive" });
    }
  };

  const onPurgeSamples = async () => {
    try {
      const n = await purgeSampleData();
      toast({
        title: n > 0 ? `Removed ${n} sample events` : "No sample data to remove",
        variant: "success",
      });
    } catch (e: any) {
      toast({ title: "Failed to remove sample data", description: String(e), variant: "destructive" });
    }
  };

  const onResetAll = async () => {
    const ok = await confirmDialog(
      "Reset TokenLens?\n\n" +
      "This will permanently delete:\n" +
      "  - all usage events, sessions, daily aggregates, alerts\n" +
      "  - all configured sources and file offsets\n" +
      "  - all settings (budgets, preferences)\n" +
      "  - all projects and pricing history\n\n" +
      "Model pricing reference data is kept. This cannot be undone.",
      { title: "Reset all data", kind: "warning" }
    );
    if (!ok) return;
    try {
      const s = await resetAllData();
      const total = s.events + s.sessions + s.daily_usage + s.alerts +
                    s.file_offsets + s.inbox_files + s.projects +
                    s.pricing_history + s.sources + s.settings;
      toast({
        title: total > 0 ? `Reset complete (${total} rows cleared)` : "Nothing to reset",
        description:
          `${s.events} events, ${s.sessions} sessions, ${s.sources} sources, ${s.settings} settings`,
        variant: "destructive",
      });
    } catch (e: any) {
      toast({ title: "Reset failed", description: String(e), variant: "destructive" });
    }
  };

  const onRecalc = async () => {
    try {
      const n = await recalculateCosts();
      toast({ title: `Recalculated ${n} events`, variant: "success" });
    } catch (e: any) {
      toast({ title: "Failed to recalc", description: String(e), variant: "destructive" });
    }
  };

  return (
    <header className="h-14 flex items-center gap-2 px-4 border-b bg-card/30">
      <div className="md:hidden flex items-center gap-2">
        <div className="h-7 w-7 rounded-md bg-gradient-to-br from-teal-400 to-cyan-600 flex items-center justify-center">
          <Sparkles className="h-4 w-4 text-white" />
        </div>
        <span className="font-semibold text-sm">TokenLens</span>
      </div>
      <div className="ml-auto flex items-center gap-2">
        <div className="hidden sm:flex items-center gap-1.5 text-xs text-muted-foreground">
          <Database className="h-3.5 w-3.5" />
          <span>{isTauri ? "Tauri" : "Mock"}</span>
        </div>
        <Select
          value={range}
          onValueChange={setRange}
          className="h-8 w-[110px] text-xs"
          options={[
            { value: "1d", label: "Last 1 day" },
            { value: "7d", label: "Last 7 days" },
            { value: "30d", label: "Last 30 days" },
            { value: "90d", label: "Last 90 days" },
            { value: "all", label: "All time" },
          ]}
        />
        <Button variant="ghost" size="icon" onClick={onRecalc} title="Recalculate costs">
          <RefreshCw className="h-4 w-4" />
        </Button>
        <Button variant="outline" size="sm" onClick={onGenerateSamples}>
          <Sparkles className="h-3.5 w-3.5" />
          Samples
        </Button>
        <Button variant="ghost" size="icon" onClick={onPurgeSamples} title="Remove sample data">
          <Trash2 className="h-4 w-4" />
        </Button>
        <Button variant="ghost" size="icon" onClick={onResetAll} title="Reset all data" className="text-destructive hover:text-destructive">
          <RotateCcw className="h-4 w-4" />
        </Button>
        <div className="flex items-center gap-0.5 ml-1 p-0.5 rounded-md border bg-card">
          {[
            { v: "light", Icon: Sun },
            { v: "dark", Icon: Moon },
            { v: "system", Icon: Monitor },
          ].map(({ v, Icon }) => (
            <button
              key={v}
              onClick={() => setTheme(v as any)}
              className={cn(
                "h-7 w-7 rounded flex items-center justify-center transition-colors",
                theme === v ? "bg-muted text-foreground" : "text-muted-foreground hover:text-foreground"
              )}
            >
              <Icon className="h-3.5 w-3.5" />
            </button>
          ))}
        </div>
      </div>
    </header>
  );
}
