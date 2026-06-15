import { create } from "zustand";

/** Bump after DB mutations so analytics pages reload without a manual filter change. */
export const useDataRevision = create<{
  revision: number;
  bump: () => void;
}>((set) => ({
  revision: 0,
  bump: () => set((s) => ({ revision: s.revision + 1 })),
}));
