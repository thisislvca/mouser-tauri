import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "../../lib/utils";

const buttonVariants = cva(
  [
    "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-2xl text-sm font-semibold",
    "transition-all duration-200 ease-out outline-none",
    "disabled:pointer-events-none disabled:opacity-50",
    "focus-visible:ring-2 focus-visible:ring-[var(--ring)] focus-visible:ring-offset-2 focus-visible:ring-offset-[var(--surface)]",
  ],
  {
    variants: {
      variant: {
        default:
          "bg-[var(--accent)] text-white shadow-[0_16px_32px_rgba(37,99,235,0.22)] hover:bg-[color-mix(in_srgb,var(--accent)_88%,black)]",
        secondary:
          "bg-[var(--sidebar-surface)] text-[var(--foreground)] hover:bg-[var(--muted)]",
        outline:
          "border border-[var(--border)] bg-white text-[var(--foreground)] hover:bg-[var(--muted)]",
        ghost:
          "text-[var(--muted-foreground)] hover:bg-[var(--muted)] hover:text-[var(--foreground)]",
        destructive:
          "border border-[#f2c9c9] bg-white text-[#973636] hover:bg-[#fff5f5]",
      },
      size: {
        default: "h-11 px-4 py-2.5",
        sm: "h-9 rounded-xl px-3",
        lg: "h-12 rounded-2xl px-5",
        icon: "h-10 w-10 rounded-full p-0",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

type ButtonProps = React.ButtonHTMLAttributes<HTMLButtonElement> &
  VariantProps<typeof buttonVariants>;

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, size, variant, type = "button", ...props }, ref) => (
    <button
      className={cn(buttonVariants({ className, size, variant }))}
      ref={ref}
      type={type}
      {...props}
    />
  ),
);

Button.displayName = "Button";

export { Button, buttonVariants };
