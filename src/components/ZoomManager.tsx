import { useEffect } from "react";
import { useZoom } from "@/stores/zoom";

function isEditableTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName;
  if (tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT") return true;
  return target.isContentEditable;
}

export function ZoomManager() {
  const zoomIn = useZoom((s) => s.zoomIn);
  const zoomOut = useZoom((s) => s.zoomOut);
  const resetZoom = useZoom((s) => s.resetZoom);

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (!e.ctrlKey && !e.metaKey) return;
      if (isEditableTarget(e.target)) return;

      const zoomInKey = e.key === "+" || e.key === "=" || e.code === "Equal" || e.code === "NumpadAdd";
      const zoomOutKey = e.key === "-" || e.key === "_" || e.code === "Minus" || e.code === "NumpadSubtract";
      const resetKey = e.key === "0" || e.code === "Digit0" || e.code === "Numpad0";

      if (zoomInKey) {
        e.preventDefault();
        zoomIn();
      } else if (zoomOutKey) {
        e.preventDefault();
        zoomOut();
      } else if (resetKey) {
        e.preventDefault();
        resetZoom();
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [zoomIn, zoomOut, resetZoom]);

  return null;
}
