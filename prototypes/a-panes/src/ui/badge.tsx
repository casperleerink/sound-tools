import { cva, type VariantProps } from "class-variance-authority";
import type * as React from "react";
import { cn } from "./cn";

const badgeVariants = cva(
  "inline-flex shrink-0 items-center justify-center gap-1 whitespace-nowrap font-medium [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        primary: "bg-gray-950 text-gray-50",
        subtle: "bg-alpha/5 text-gray-950",
        outline: "border border-alpha/10 text-gray-950",
        ghost: "text-gray-950/60",
        agent: "bg-lavender-500/10 text-lavender-500",
        green: "bg-green-500/10 text-green-500",
        orange: "bg-orange-500/10 text-orange-500",
        red: "bg-red-500/10 text-red-500",
        blue: "bg-blue-500/10 text-blue-500",
        accent: "bg-(--accent)/10 text-(--accent)",
      },
      size: {
        xs: "h-4 rounded px-1 text-2xs [&_svg]:size-2.5",
        sm: "h-5 rounded px-1.5 text-xs [&_svg]:size-3",
        md: "h-6 rounded-md px-2 text-xs [&_svg]:size-3.5",
        lg: "h-7 rounded-md px-2 text-sm [&_svg]:size-4",
      },
    },
    defaultVariants: { variant: "subtle", size: "sm" },
  },
);

export interface BadgeProps
  extends React.HTMLAttributes<HTMLSpanElement>,
    VariantProps<typeof badgeVariants> {
  icon?: React.ReactNode;
  /** Numbers and times: use tabular figures. */
  tabular?: boolean;
}

export function Badge({ className, variant, size, icon, tabular, children, ...props }: BadgeProps) {
  return (
    <span className={cn(badgeVariants({ variant, size }), tabular && "tabular", className)} {...props}>
      {icon}
      {children}
    </span>
  );
}

/** Small coloured dot, used for tool accents and status. */
export function Dot({
  className,
  color,
  pulse,
}: {
  className?: string;
  /** CSS colour; defaults to the current --accent. */
  color?: string;
  pulse?: boolean;
}) {
  return (
    <span
      aria-hidden
      className={cn("inline-block size-2 shrink-0 rounded-full", pulse && "animate-blink-dot", className)}
      style={{ background: color ?? "var(--accent)" }}
    />
  );
}

export { badgeVariants };
