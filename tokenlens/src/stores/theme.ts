// Theme provider — minimal light/dark/system implementation.
import { create } from "zustand";
import { persist } from "zustand/middleware";

type Theme = "light" | "dark" | "system";
interface ThemeState {
  theme: Theme;
  resolved: "light" | "dark";
  setTheme: (t: Theme) => void;
}

const computeResolved = (t: Theme): "light" | "dark" => {
  if (t === "system") {
    return window.matchMedia?.("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }
  return t;
};

const apply = (resolved: "light" | "dark") => {
  const root = document.documentElement;
  root.classList.remove("light", "dark");
  root.classList.add(resolved);
};

export const useTheme = create<ThemeState>()(
  persist(
    (set, get) => ({
      theme: "system",
      resolved: typeof window !== "undefined" ? computeResolved("system") : "light",
      setTheme: (t) => {
        const resolved = computeResolved(t);
        apply(resolved);
        set({ theme: t, resolved });
      },
    }),
    { name: "tokenlens-theme" }
  )
);

// Initialize on app boot.
if (typeof window !== "undefined") {
  const t = useTheme.getState().theme;
  const resolved = computeResolved(t);
  apply(resolved);
  useTheme.setState({ resolved });
  window.matchMedia?.("(prefers-color-scheme: dark)").addEventListener?.("change", () => {
    if (useTheme.getState().theme === "system") {
      const r = computeResolved("system");
      apply(r);
      useTheme.setState({ resolved: r });
    }
  });
}
