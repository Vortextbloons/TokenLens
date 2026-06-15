import { Sun, Moon, Monitor, Sparkles, Database, RefreshCw } from "lucide-react";
import { useTheme } from "@/stores/theme";
import { useFilter } from "@/stores/filter";
import { Button } from "@/components/ui/primitives";
import { Select } from "@/components/ui/primitives";
import { cn } from "@/lib/utils";
import { toast } from "@/stores/toast";
import { generateSampleData, isTauri, recalculateCosts } from "@/lib/tauri";

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
