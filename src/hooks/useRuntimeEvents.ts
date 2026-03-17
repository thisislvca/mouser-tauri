import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useQueryClient } from "@tanstack/react-query";
import type { DebugEventRecord } from "../lib/types";
import { useUiStore } from "../store/uiStore";

export function useRuntimeEvents() {
  const queryClient = useQueryClient();
  const appendDebugEvent = useUiStore((state) => state.appendDebugEvent);

  useEffect(() => {
    let cancelled = false;
    const unlisten: Array<() => void> = [];

    async function bind() {
      const invalidate = () => {
        void queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
      };

      const listeners = await Promise.all([
        listen("device_changed", invalidate),
        listen("profile_changed", invalidate),
        listen("engine_status_changed", invalidate),
        listen<DebugEventRecord>("debug_event", (event) => {
          appendDebugEvent(event.payload);
        }),
      ]);

      if (cancelled) {
        listeners.forEach((stop) => stop());
        return;
      }

      unlisten.push(...listeners);
    }

    void bind();

    return () => {
      cancelled = true;
      unlisten.forEach((stop) => stop());
    };
  }, [appendDebugEvent, queryClient]);
}
