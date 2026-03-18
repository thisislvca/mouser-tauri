import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "../../lib/utils";

const badgeVariants = cva(
  "inline-flex items-center rounded-full border px-3 py-1 text-xs font-semibold tracking-[0.01em]",
  {
    variants: {
      variant: {
        default: "border-[var(--border)] bg-white text-[var(--muted-foreground)]",
        success: "border-[#d3eadc] bg-white text-[#177a4d]",
        accent: "border-[#c6dafd] bg-white text-[var(--accent)]",
        warning: "border-[#f1dfc1] bg-white text-[#92611f]",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  },
);

type BadgeProps = React.HTMLAttributes<HTMLSpanElement> & VariantProps<typeof badgeVariants>;

function Badge({ className, variant, ...props }: BadgeProps) {
  return <span className={cn(badgeVariants({ className, variant }))} {...props} />;
}

export { Badge, badgeVariants };
