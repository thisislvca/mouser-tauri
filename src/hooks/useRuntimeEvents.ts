import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { events, type DebugEvent } from "../lib/api";
import { useUiStore } from "../store/uiStore";

export function useRuntimeEvents() {
  const queryClient = useQueryClient();
  const appendDebugEvent = useUiStore((state) => state.appendDebugEvent);

  useEffect(() => {
    let cancelled = false;
    let invalidateTimer: number | null = null;
    const unlisten: Array<() => void> = [];

    async function bind() {
      const scheduleInvalidate = () => {
        if (invalidateTimer != null) {
          return;
        }

        invalidateTimer = window.setTimeout(() => {
          invalidateTimer = null;
          void queryClient.invalidateQueries({ queryKey: ["bootstrap"] });
        }, 0);
      };

      const listeners = await Promise.all([
        events.appDiscoveryChangedEvent.listen(scheduleInvalidate),
        events.deviceChangedEvent.listen(scheduleInvalidate),
        events.profileChangedEvent.listen(scheduleInvalidate),
        events.engineStatusChangedEvent.listen(scheduleInvalidate),
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
      if (invalidateTimer != null) {
        window.clearTimeout(invalidateTimer);
      }
      unlisten.forEach((stop) => stop());
    };
  }, [appendDebugEvent, queryClient]);
}
