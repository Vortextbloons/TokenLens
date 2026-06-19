import { NavLink, useLocation } from "react-router-dom";
import {
  LayoutDashboard,
  MessagesSquare,
  FolderKanban,
  Cpu,
  Plug,
  DollarSign,
  Clock,
  ListTree,
  Settings,
  Activity,
  TrendingUp,
} from "lucide-react";
import { cn } from "@/lib/utils";

const items = [
  { to: "/", label: "Overview", icon: LayoutDashboard, end: true },
  { to: "/sessions", label: "Sessions", icon: MessagesSquare },
  { to: "/projects", label: "Projects", icon: FolderKanban },
  { to: "/models", label: "Models", icon: Cpu },
  { to: "/insights", label: "Insights", icon: TrendingUp },
  { to: "/providers", label: "Providers", icon: Plug },
  { to: "/costs", label: "Costs", icon: DollarSign },
  { to: "/timeline", label: "Timeline", icon: Clock },
  { to: "/raw-events", label: "Raw Events", icon: ListTree },
  { to: "/settings", label: "Settings", icon: Settings },
];

export function Sidebar() {
  const loc = useLocation();
  return (
    <aside className="hidden md:flex flex-col w-60 shrink-0 border-r bg-card/30">
      <div className="h-14 flex items-center gap-2 px-4 border-b">
        <div className="h-7 w-7 rounded-md bg-gradient-to-br from-teal-400 to-cyan-600 flex items-center justify-center">
          <Activity className="h-4 w-4 text-white" />
        </div>
        <div>
          <div className="font-semibold text-sm leading-none">TokenLens</div>
          <div className="text-[10px] text-muted-foreground leading-none mt-0.5">local token analytics</div>
        </div>
      </div>
      <nav className="flex-1 p-2 space-y-0.5 overflow-y-auto scrollbar-thin">
        {items.map((it) => {
          const Icon = it.icon;
          const active = it.end ? loc.pathname === it.to : loc.pathname.startsWith(it.to);
          return (
            <NavLink
              key={it.to}
              to={it.to}
              end={it.end}
              className={cn(
                "flex items-center gap-2.5 px-3 py-2 rounded-md text-sm transition-colors",
                active
                  ? "bg-primary/10 text-primary font-medium"
                  : "text-muted-foreground hover:bg-muted hover:text-foreground"
              )}
            >
              <Icon className="h-4 w-4 shrink-0" />
              <span>{it.label}</span>
            </NavLink>
          );
        })}
      </nav>
      <div className="p-3 border-t text-[10px] text-muted-foreground">
        <div>No telemetry. No AI. No cloud.</div>
        <div className="mt-1 opacity-60">v0.2.0</div>
      </div>
    </aside>
  );
}
