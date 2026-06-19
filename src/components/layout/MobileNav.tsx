import { NavLink } from "react-router-dom";
import { LayoutDashboard, MessagesSquare, Clock, Settings, TrendingUp } from "lucide-react";
import { cn } from "@/lib/utils";

const items = [
  { to: "/", label: "Home", icon: LayoutDashboard, end: true },
  { to: "/sessions", label: "Sessions", icon: MessagesSquare },
  { to: "/insights", label: "Insights", icon: TrendingUp },
  { to: "/timeline", label: "Timeline", icon: Clock },
  { to: "/settings", label: "Settings", icon: Settings },
];

/** Compact nav for viewports below md where the sidebar is hidden. */
export function MobileNav() {
  return (
    <nav className="md:hidden flex border-b bg-card/50 overflow-x-auto scrollbar-thin">
      {items.map((it) => {
        const Icon = it.icon;
        return (
          <NavLink
            key={it.to}
            to={it.to}
            end={it.end}
            className={({ isActive }) =>
              cn(
                "flex flex-col items-center gap-0.5 px-4 py-2 text-[10px] shrink-0 transition-colors",
                isActive ? "text-primary font-medium" : "text-muted-foreground"
              )
            }
          >
            <Icon className="h-4 w-4" />
            {it.label}
          </NavLink>
        );
      })}
    </nav>
  );
}
