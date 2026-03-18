import * as React from "react";
import { cn } from "../../lib/utils";

const Textarea = React.forwardRef<HTMLTextAreaElement, React.ComponentProps<"textarea">>(
  ({ className, ...props }, ref) => (
    <textarea
      className={cn(
        "flex min-h-[96px] w-full rounded-3xl border border-[var(--input)] bg-white px-4 py-3 text-sm text-[var(--foreground)] shadow-[inset_0_1px_0_rgba(255,255,255,0.8)] outline-none transition",
        "placeholder:text-[var(--muted-foreground)]",
        "focus-visible:border-[var(--accent)] focus-visible:ring-4 focus-visible:ring-[color-mix(in_srgb,var(--accent)_14%,transparent)]",
        "disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      ref={ref}
      {...props}
    />
  ),
);

Textarea.displayName = "Textarea";

export { Textarea };
