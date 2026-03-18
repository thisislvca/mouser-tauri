import * as React from "react";
import { ScrollArea as BaseScrollArea } from "@base-ui/react/scroll-area";
import { cn } from "../../lib/utils";

const ScrollArea = React.forwardRef<
  HTMLDivElement,
  React.ComponentPropsWithoutRef<typeof BaseScrollArea.Root>
>(({ className, children, ...props }, ref) => (
  <BaseScrollArea.Root className={cn("relative overflow-hidden", className)} ref={ref} {...props}>
    <BaseScrollArea.Viewport className="h-full w-full rounded-[inherit]">
      <BaseScrollArea.Content>{children}</BaseScrollArea.Content>
    </BaseScrollArea.Viewport>
    <BaseScrollArea.Scrollbar
      className="flex w-2 touch-none select-none rounded-full bg-transparent p-0.5"
      orientation="vertical"
    >
      <BaseScrollArea.Thumb className="flex-1 rounded-full bg-[var(--border-strong)]" />
    </BaseScrollArea.Scrollbar>
    <BaseScrollArea.Scrollbar
      className="flex h-2 touch-none select-none rounded-full bg-transparent p-0.5"
      orientation="horizontal"
    >
      <BaseScrollArea.Thumb className="flex-1 rounded-full bg-[var(--border-strong)]" />
    </BaseScrollArea.Scrollbar>
  </BaseScrollArea.Root>
));

ScrollArea.displayName = "ScrollArea";

export { ScrollArea };
