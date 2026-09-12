import { cva, type VariantProps } from "class-variance-authority";
import * as React from "react";
import { cn } from "./cn";

const disabled =
  "disabled:pointer-events-none disabled:opacity-40 aria-disabled:pointer-events-none aria-disabled:opacity-40";

const buttonVariants = cva(
  "relative inline-flex shrink-0 select-none items-center justify-center whitespace-nowrap font-medium transition-colors duration-100 [&_svg]:pointer-events-none [&_svg]:shrink-0",
  {
    variants: {
      variant: {
        primary: `bg-gray-950 text-gray-50 hover:bg-gray-900 ${disabled}`,
        subtle: `bg-alpha/5 text-gray-950 hover:bg-alpha/10 ${disabled}`,
        outline: `border border-alpha/10 bg-alpha/5 text-gray-950 hover:bg-alpha/10 ${disabled}`,
        ghost: `bg-transparent text-gray-950 hover:bg-alpha/5 ${disabled}`,
        /* muted ghost: for dense chrome, dims until hovered */
        quiet: `bg-transparent text-gray-950/60 hover:bg-alpha/5 hover:text-gray-950 ${disabled}`,

        agent: `bg-lavender-500 text-gray-200 hover:bg-lavender-500/90 ${disabled}`,
        "agent-subtle": `bg-lavender-500/10 text-lavender-500 hover:bg-lavender-500/20 ${disabled}`,
        "agent-ghost": `bg-transparent text-lavender-500 hover:bg-lavender-500/10 ${disabled}`,

        green: `bg-green-500 text-gray-200 hover:bg-green-500/90 ${disabled}`,
        "green-subtle": `bg-green-500/10 text-green-500 hover:bg-green-500/20 ${disabled}`,
        "green-ghost": `bg-transparent text-green-500 hover:bg-green-500/10 ${disabled}`,

        orange: `bg-orange-500 text-gray-200 hover:bg-orange-500/90 ${disabled}`,
        "orange-subtle": `bg-orange-500/10 text-orange-500 hover:bg-orange-500/20 ${disabled}`,
        "orange-ghost": `bg-transparent text-orange-500 hover:bg-orange-500/10 ${disabled}`,

        red: `bg-red-500 text-gray-200 hover:bg-red-500/90 ${disabled}`,
        "red-subtle": `bg-red-500/10 text-red-500 hover:bg-red-500/20 ${disabled}`,
        "red-ghost": `bg-transparent text-red-500 hover:bg-red-500/10 ${disabled}`,

        /* Follows the tool instance accent set on a ToolView frame (--accent). */
        accent: `bg-(--accent) text-gray-200 hover:opacity-90 ${disabled}`,
        "accent-subtle": `bg-(--accent)/10 text-(--accent) hover:bg-(--accent)/20 ${disabled}`,
        "accent-ghost": `bg-transparent text-(--accent) hover:bg-(--accent)/10 ${disabled}`,
      },
      size: {
        xs: "h-6 gap-0.5 rounded-md px-1.5 text-xs [&_svg]:size-3.5",
        sm: "h-7 gap-1 rounded-md px-2 text-sm [&_svg]:size-4",
        md: "h-8 gap-1 rounded-lg px-2.5 text-sm [&_svg]:size-4",
        lg: "h-10 gap-2 rounded-[10px] px-3 text-base font-semibold [&_svg]:size-4",
        "icon-xs": "size-6 rounded-md [&_svg]:size-3.5",
        "icon-sm": "size-7 rounded-md [&_svg]:size-4",
        "icon-md": "size-8 rounded-lg [&_svg]:size-4",
        "icon-lg": "size-10 rounded-[10px] [&_svg]:size-5",
      },
      active: {
        true: "bg-alpha/10 text-gray-950",
        false: "",
      },
    },
    defaultVariants: {
      variant: "subtle",
      size: "md",
      active: false,
    },
  },
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, active, type = "button", ...props }, ref) => (
    <button
      ref={ref}
      type={type}
      className={cn(buttonVariants({ variant, size, active }), className)}
      {...props}
    />
  ),
);
Button.displayName = "Button";

export interface IconButtonProps extends Omit<ButtonProps, "size" | "children"> {
  /** Accessible name. Also used as the tooltip. */
  label: string;
  size?: "xs" | "sm" | "md" | "lg";
  children: React.ReactNode;
}

/** Icon-only button. Always labelled. */
export const IconButton = React.forwardRef<HTMLButtonElement, IconButtonProps>(
  ({ label, size = "md", variant = "ghost", className, children, ...props }, ref) => (
    <Button
      ref={ref}
      aria-label={label}
      title={label}
      variant={variant}
      size={`icon-${size}`}
      className={className}
      {...props}
    >
      {children}
    </Button>
  ),
);
IconButton.displayName = "IconButton";

export { buttonVariants };
