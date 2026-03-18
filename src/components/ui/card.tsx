import * as React from "react";
import { cn } from "../../lib/utils";

const Card = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => (
    <div
      className={cn(
        "rounded-[28px] bg-[var(--card)] text-[var(--card-foreground)] shadow-[0_24px_60px_rgba(15,23,42,0.07)] ring-1 ring-[var(--border-soft)]",
        className,
      )}
      ref={ref}
      {...props}
    />
  ),
);

const CardHeader = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => (
    <div className={cn("flex flex-col gap-2 p-6", className)} ref={ref} {...props} />
  ),
);

const CardTitle = React.forwardRef<HTMLParagraphElement, React.HTMLAttributes<HTMLHeadingElement>>(
  ({ className, ...props }, ref) => (
    <h3
      className={cn("text-[22px] font-semibold tracking-[-0.04em] text-[var(--foreground)]", className)}
      ref={ref}
      {...props}
    />
  ),
);

const CardDescription = React.forwardRef<
  HTMLParagraphElement,
  React.HTMLAttributes<HTMLParagraphElement>
>(({ className, ...props }, ref) => (
  <p
    className={cn("text-sm leading-6 text-[var(--muted-foreground)]", className)}
    ref={ref}
    {...props}
  />
));

const CardContent = React.forwardRef<HTMLDivElement, React.HTMLAttributes<HTMLDivElement>>(
  ({ className, ...props }, ref) => (
    <div className={cn("px-6 pb-6", className)} ref={ref} {...props} />
  ),
);

Card.displayName = "Card";
CardHeader.displayName = "CardHeader";
CardTitle.displayName = "CardTitle";
CardDescription.displayName = "CardDescription";
CardContent.displayName = "CardContent";

export { Card, CardContent, CardDescription, CardHeader, CardTitle };
