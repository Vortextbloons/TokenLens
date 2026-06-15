import { create } from "zustand";
import { persist } from "zustand/middleware";

export const ZOOM_MIN = 0.75;
export const ZOOM_MAX = 1.75;
export const ZOOM_STEP = 0.1;
export const ZOOM_DEFAULT = 1;

const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

function clampZoom(value: number): number {
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, Math.round(value * 100) / 100));
}

export async function applyZoom(scale: number): Promise<void> {
  const zoom = clampZoom(scale);
  if (inTauri) {
    try {
      const { getCurrentWebviewWindow } = await import("@tauri-apps/api/webviewWindow");
      await getCurrentWebviewWindow().setZoom(zoom);
    } catch {
      document.documentElement.style.zoom = String(zoom);
    }
  } else {
    document.documentElement.style.zoom = String(zoom);
  }
}

interface ZoomState {
  zoom: number;
  setZoom: (value: number) => void;
  zoomIn: () => void;
  zoomOut: () => void;
  resetZoom: () => void;
}

export const useZoom = create<ZoomState>()(
  persist(
    (set, get) => ({
      zoom: ZOOM_DEFAULT,
      setZoom: (value) => {
        const zoom = clampZoom(value);
        void applyZoom(zoom);
        set({ zoom });
      },
      zoomIn: () => get().setZoom(get().zoom + ZOOM_STEP),
      zoomOut: () => get().setZoom(get().zoom - ZOOM_STEP),
      resetZoom: () => get().setZoom(ZOOM_DEFAULT),
    }),
    { name: "tokenlens-zoom" },
  ),
);

if (typeof window !== "undefined") {
  void applyZoom(useZoom.getState().zoom);
  useZoom.persist.onFinishHydration((state) => {
    void applyZoom(state.zoom);
  });
}
