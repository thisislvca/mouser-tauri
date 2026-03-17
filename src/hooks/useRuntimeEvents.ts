import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { events, type DebugEvent } from "../lib/api";
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
        events.deviceChangedEvent.listen(invalidate),
        events.profileChangedEvent.listen(invalidate),
        events.engineStatusChangedEvent.listen(invalidate),
        events.debugEventEnvelope.listen((event: { payload: DebugEvent }) => {
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
