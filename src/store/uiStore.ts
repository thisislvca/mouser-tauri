import { create } from "zustand";

export type SectionName = "devices" | "buttons" | "profiles" | "debug";
export type ShellMode = "dashboard" | "detail";

interface UiState {
  shellMode: ShellMode;
  activeSection: SectionName;
  selectedProfileId: string | null;
  importDraft: string;
  setShellMode: (mode: ShellMode) => void;
  setActiveSection: (section: SectionName) => void;
  setSelectedProfileId: (profileId: string | null) => void;
  setImportDraft: (value: string) => void;
}

export const useUiStore = create<UiState>((set) => ({
  shellMode: "dashboard",
  activeSection: "buttons",
  selectedProfileId: null,
  importDraft: "",
  setShellMode: (shellMode) => set({ shellMode }),
  setActiveSection: (activeSection) => set({ activeSection }),
  setSelectedProfileId: (selectedProfileId) => set({ selectedProfileId }),
  setImportDraft: (importDraft) => set({ importDraft }),
}));
