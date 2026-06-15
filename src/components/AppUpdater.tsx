import { useEffect } from "react";
import { isTauri, confirmDialog } from "@/lib/tauri";

export function AppUpdater() {
  useEffect(() => {
    if (!isTauri) return;

    let cancelled = false;

    (async () => {
      try {
        const { check } = await import("@tauri-apps/plugin-updater");
        const update = await check();
        if (!update || cancelled) return;

        const accepted = await confirmDialog(
          `A new version (${update.version}) is available. Install now?`,
          { title: "TokenLens Update", kind: "info" }
        );
        if (!accepted || cancelled) return;

        await update.downloadAndInstall();
        const { relaunch } = await import("@tauri-apps/plugin-process");
        await relaunch();
      } catch (err) {
        console.debug("[AppUpdater] check failed:", err);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, []);

  return null;
}
