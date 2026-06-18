// Global app stores: filter range, etc.
import { create } from "zustand";
import { persist } from "zustand/middleware";
import { useMemo } from "react";
import { rangeToDates, localDateString } from "@/lib/utils";
import type { QueryFilter } from "@/types/contracts";

interface FilterState {
  range: string; // "1d" | "7d" | "30d" | "90d" | "all"
  provider: string | null;
  model: string | null;
  setRange: (r: string) => void;
  setProvider: (p: string | null) => void;
  setModel: (m: string | null) => void;
}

export const useFilter = create<FilterState>()(
  persist(
    (set) => ({
      range: "7d",
      provider: null,
      model: null,
      setRange: (r) => set({ range: r }),
      setProvider: (p) => set({ provider: p }),
      setModel: (m) => set({ model: m }),
    }),
    { name: "tokenlens-filter" }
  )
);

/**
 * Build a stable QueryFilter object from the current store state.
 *
 * IMPORTANT: this hook uses three independent primitive selectors (range,
 * provider, model) instead of selecting the whole object at once. That way
 * Zustand's reference equality is checked against primitives, which never
 * triggers spurious re-renders. The resulting QueryFilter is then memoized
 * so the object identity is stable across renders — safe to use as a
 * useEffect dependency.
 */
export function useFilterObject(): QueryFilter {
  const range = useFilter((s) => s.range);
  const provider = useFilter((s) => s.provider);
  const model = useFilter((s) => s.model);

  return useMemo(() => {
    const { start, end } = rangeToDates(range);
    const filter: QueryFilter = {
      start_date: start,
      end_date: end,
      local_date: localDateString(new Date()),
      provider,
      model,
    };
    return filter;
  }, [range, provider, model]);
}
