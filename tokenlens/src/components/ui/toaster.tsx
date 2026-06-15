import { useEffect } from "react";
import { useToasts } from "@/stores/toast";
import { cn } from "@/lib/utils";
import { CheckCircle2, AlertCircle, X } from "lucide-react";

export function Toaster() {
  const toasts = useToasts((s) => s.toasts);
  const dismiss = useToasts((s) => s.dismiss);

  useEffect(() => {
    // no-op
  }, [toasts.length]);

  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-50 flex flex-col gap-2 w-[360px] max-w-full">
      {toasts.map((t) => (
        <div
          key={t.id}
          className={cn(
            "pointer-events-auto flex items-start gap-3 rounded-lg border bg-card p-3 shadow-lg animate-fade-in",
            t.variant === "destructive" && "border-destructive/50",
            t.variant === "success" && "border-emerald-500/40"
          )}
        >
          {t.variant === "destructive" ? (
            <AlertCircle className="h-4 w-4 text-destructive mt-0.5" />
          ) : (
            <CheckCircle2 className="h-4 w-4 text-emerald-500 mt-0.5" />
          )}
          <div className="flex-1 min-w-0">
            <div className="text-sm font-medium">{t.title}</div>
            {t.description ? (
              <div className="text-xs text-muted-foreground mt-0.5">{t.description}</div>
            ) : null}
          </div>
          <button onClick={() => dismiss(t.id)} className="text-muted-foreground hover:text-foreground">
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
      ))}
    </div>
  );
}
