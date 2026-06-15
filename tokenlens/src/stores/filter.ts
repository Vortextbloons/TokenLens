// Global app stores: filter range, etc.
import { create } from "zustand";
import { persist } from "zustand/middleware";
import { rangeToDates } from "@/lib/utils";
import type { QueryFilter } from "@/types/contracts";

interface FilterState {
  range: string; // "1d" | "7d" | "30d" | "90d" | "all"
  provider: string | null;
  model: string | null;
  setRange: (r: string) => void;
  setProvider: (p: string | null) => void;
  setModel: (m: string | null) => void;
  toFilter: () => QueryFilter;
}

export const useFilter = create<FilterState>()(
  persist(
    (set, get) => ({
      range: "7d",
      provider: null,
      model: null,
      setRange: (r) => set({ range: r }),
      setProvider: (p) => set({ provider: p }),
      setModel: (m) => set({ model: m }),
      toFilter: () => {
        const s = get();
        const { start, end } = rangeToDates(s.range);
        return {
          start_date: start,
          end_date: end,
          provider: s.provider,
          model: s.model,
        };
      },
    }),
    { name: "tokenlens-filter" }
  )
);
