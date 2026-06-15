import { HashRouter, Routes, Route, Navigate } from "react-router-dom";
import { Sidebar } from "@/components/layout/Sidebar";
import { Topbar } from "@/components/layout/Topbar";
import { Toaster } from "@/components/ui/toaster";
import { AppUpdater } from "@/components/AppUpdater";
import { Overview } from "@/pages/Overview";
import { Sessions } from "@/pages/Sessions";
import { SessionDetail } from "@/pages/SessionDetail";
import { Projects } from "@/pages/Projects";
import { Models } from "@/pages/Models";
import { Providers } from "@/pages/Providers";
import { Costs } from "@/pages/Costs";
import { Timeline } from "@/pages/Timeline";
import { RawEvents } from "@/pages/RawEvents";
import { Settings } from "@/pages/Settings";
import { MobileNav } from "@/components/layout/MobileNav";
import { ZoomManager } from "@/components/ZoomManager";

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
              <Routes>
                <Route path="/" element={<Overview />} />
                <Route path="/sessions" element={<Sessions />} />
                <Route path="/sessions/:id" element={<SessionDetail />} />
                <Route path="/projects" element={<Projects />} />
                <Route path="/models" element={<Models />} />
                <Route path="/providers" element={<Providers />} />
                <Route path="/costs" element={<Costs />} />
                <Route path="/timeline" element={<Timeline />} />
                <Route path="/raw-events" element={<RawEvents />} />
                <Route path="/settings" element={<Settings />} />
                <Route path="*" element={<Navigate to="/" replace />} />
              </Routes>
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
