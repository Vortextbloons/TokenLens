import { HashRouter, Routes, Route, Navigate } from "react-router-dom";
import { lazy, Suspense } from "react";
import { Sidebar } from "@/components/layout/Sidebar";
import { Topbar } from "@/components/layout/Topbar";
import { Toaster } from "@/components/ui/toaster";
import { AppUpdater } from "@/components/AppUpdater";
import { MobileNav } from "@/components/layout/MobileNav";
import { ZoomManager } from "@/components/ZoomManager";
import { Skeleton } from "@/components/ui/primitives";

// Every page is lazy-loaded. The default route (`/`) renders Overview, but
// Vite still traces the static `import` for `modulepreload`, so we wrap
// the default route in the same lazy() pattern to keep recharts out of the
// critical-path modulepreload list.
const Overview = lazy(() => import("@/pages/Overview").then((m) => ({ default: m.Overview })));
const Sessions = lazy(() => import("@/pages/Sessions").then((m) => ({ default: m.Sessions })));
const SessionDetail = lazy(() =>
  import("@/pages/SessionDetail").then((m) => ({ default: m.SessionDetail })),
);
const Projects = lazy(() => import("@/pages/Projects").then((m) => ({ default: m.Projects })));
const Models = lazy(() => import("@/pages/Models").then((m) => ({ default: m.Models })));
const Insights = lazy(() => import("@/pages/Insights").then((m) => ({ default: m.Insights })));
const Providers = lazy(() =>
  import("@/pages/Providers").then((m) => ({ default: m.Providers })),
);
const Costs = lazy(() => import("@/pages/Costs").then((m) => ({ default: m.Costs })));
const Timeline = lazy(() => import("@/pages/Timeline").then((m) => ({ default: m.Timeline })));
const RawEvents = lazy(() => import("@/pages/RawEvents").then((m) => ({ default: m.RawEvents })));
const Settings = lazy(() => import("@/pages/Settings").then((m) => ({ default: m.Settings })));

function RouteFallback() {
  return (
    <div className="p-4 space-y-3">
      <Skeleton className="h-10 w-1/3" />
      <Skeleton className="h-72" />
    </div>
  );
}

export default function App() {
  return (
    <HashRouter
      future={{
        v7_startTransition: true,
        v7_relativeSplatPath: true,
      }}
    >
      <div className="flex h-full bg-background text-foreground">
        <Sidebar />
        <div className="flex-1 flex flex-col min-w-0">
          <Topbar />
          <MobileNav />
          <main className="flex-1 overflow-y-auto scrollbar-thin">
            <div className="p-4 md:p-6 w-full min-w-0">
              <Suspense fallback={<RouteFallback />}>
                <Routes>
                  <Route path="/" element={<Overview />} />
                  <Route path="/sessions" element={<Sessions />} />
                  <Route path="/sessions/:id" element={<SessionDetail />} />
                  <Route path="/projects" element={<Projects />} />
                  <Route path="/models" element={<Models />} />
                  <Route path="/insights" element={<Insights />} />
                  <Route path="/providers" element={<Providers />} />
                  <Route path="/costs" element={<Costs />} />
                  <Route path="/timeline" element={<Timeline />} />
                  <Route path="/raw-events" element={<RawEvents />} />
                  <Route path="/settings" element={<Settings />} />
                  <Route path="*" element={<Navigate to="/" replace />} />
                </Routes>
              </Suspense>
            </div>
          </main>
        </div>
        <Toaster />
        <AppUpdater />
        <ZoomManager />
      </div>
    </HashRouter>
  );
}
