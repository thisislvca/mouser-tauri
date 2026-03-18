import { create } from "zustand";
import type { DebugEventRecord } from "../lib/types";

export type SectionName = "devices" | "buttons" | "profiles" | "debug";
export type ShellMode = "dashboard" | "detail";

interface UiState {
  shellMode: ShellMode;
  activeSection: SectionName;
  selectedProfileId: string | null;
  importDraft: string;
  eventLog: DebugEventRecord[];
  setShellMode: (mode: ShellMode) => void;
  setActiveSection: (section: SectionName) => void;
  setSelectedProfileId: (profileId: string | null) => void;
  setImportDraft: (value: string) => void;
  appendDebugEvent: (event: DebugEventRecord) => void;
  hydrateDebugLog: (events: DebugEventRecord[]) => void;
  clearDebugEvents: () => void;
}

export const useUiStore = create<UiState>((set) => ({
  shellMode: "dashboard",
  activeSection: "devices",
  selectedProfileId: null,
  importDraft: "",
  eventLog: [],
  setShellMode: (shellMode) => set({ shellMode }),
  setActiveSection: (activeSection) => set({ activeSection }),
  setSelectedProfileId: (selectedProfileId) => set({ selectedProfileId }),
  setImportDraft: (importDraft) => set({ importDraft }),
  appendDebugEvent: (event) =>
    set((state) => ({
      eventLog: [event, ...state.eventLog].slice(0, 24),
    })),
  hydrateDebugLog: (events) => set({ eventLog: events.slice(0, 24) }),
  clearDebugEvents: () => set({ eventLog: [] }),
}));
