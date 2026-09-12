import { cva, type VariantProps } from "class-variance-authority";
import type * as React from "react";
import { cn } from "@/lib/cn";

const badgeVariants = cva(
  "inline-flex shrink-0 items-center justify-center gap-1 whitespace-nowrap font-medium [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        primary: "bg-gray-950 text-ink",
        subtle: "bg-alpha/5 text-gray-950",
        muted: "bg-alpha/5 text-gray-800",
        outline: "border border-alpha/10 text-gray-900",
        accent: "bg-(--accent) text-ink",
        "accent-subtle": "bg-(--accent)/12 text-(--accent)",
        "accent-outline": "border border-(--accent)/40 text-(--accent)",
        agent: "bg-lavender-500/12 text-lavender-500",
        green: "bg-green-500/12 text-green-500",
        orange: "bg-orange-500/12 text-orange-500",
        red: "bg-red-500/12 text-red-500",
        blue: "bg-blue-500/12 text-blue-500",
        yellow: "bg-yellow-500/12 text-yellow-500",
      },
      size: {
        xs: "h-4 rounded px-1 text-2xs [&_svg]:size-2.5",
        sm: "h-5 rounded-md px-1.5 text-xs [&_svg]:size-3",
        md: "h-6 rounded-md px-2 text-xs [&_svg]:size-3.5",
        lg: "h-7 rounded-lg px-2 text-sm [&_svg]:size-4",
      },
      rounded: { true: "rounded-full", false: "" },
    },
    defaultVariants: { variant: "subtle", size: "sm", rounded: false },
  },
);

export interface BadgeProps
  extends React.HTMLAttributes<HTMLSpanElement>,
    VariantProps<typeof badgeVariants> {
  icon?: React.ReactNode;
  /** Small dot in the current text colour before the label. */
  dot?: boolean;
}

export function Badge({ className, variant, size, rounded, icon, dot, children, ...props }: BadgeProps) {
  return (
    <span className={cn(badgeVariants({ variant, size, rounded }), className)} {...props}>
      {dot ? <span aria-hidden className="size-1.5 rounded-full bg-current" /> : null}
      {icon ?? null}
      {children}
    </span>
  );
}

export { badgeVariants };
