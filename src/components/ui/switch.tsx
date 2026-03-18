import * as React from "react";
import { Switch as BaseSwitch } from "@base-ui/react/switch";
import { cn } from "../../lib/utils";

const Switch = React.forwardRef<
  HTMLElement,
  React.ComponentPropsWithoutRef<typeof BaseSwitch.Root>
>(({ className, checked, onCheckedChange, ...props }, ref) => (
  <BaseSwitch.Root
    checked={checked}
    className={cn(
      "relative inline-flex h-7 w-12 items-center rounded-full border border-transparent p-0.5 outline-none transition",
      "data-[checked]:bg-[var(--accent)] data-[checked=false]:bg-[var(--border-strong)]",
      "focus-visible:ring-4 focus-visible:ring-[color-mix(in_srgb,var(--accent)_14%,transparent)]",
      className,
    )}
    onCheckedChange={onCheckedChange}
    ref={ref}
    {...props}
  >
    <BaseSwitch.Thumb
      className={cn(
        "block h-5 w-5 rounded-full bg-white shadow-[0_6px_16px_rgba(15,23,42,0.18)] transition-transform duration-200",
        "data-[checked]:translate-x-5 data-[checked=false]:translate-x-0",
      )}
    />
  </BaseSwitch.Root>
));

Switch.displayName = "Switch";

export { Switch };
