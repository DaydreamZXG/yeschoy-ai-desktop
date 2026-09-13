import * as React from "react";
import { Slot } from "@radix-ui/react-slot";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";
import "./ui-primitives.css";

const buttonVariants = cva(
  "ui-btn",
  {
    variants: {
      variant: {
        default: "ui-btn--default",
        destructive: "ui-btn--destructive",
        outline: "ui-btn--outline",
        secondary: "ui-btn--secondary",
        ghost: "ui-btn--ghost",
        mcp: "ui-btn--mcp",
        link: "ui-btn--link",
      },
      size: {
        default: "",
        sm: "ui-btn--sm",
        lg: "ui-btn--lg",
        icon: "ui-btn--icon",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {
  asChild?: boolean;
}

const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, asChild = false, ...props }, ref) => {
    const Comp = asChild ? Slot : "button";
    return (
      <Comp
        className={cn(buttonVariants({ variant, size, className }))}
        ref={ref}
        {...props}
      />
    );
  },
);
Button.displayName = "Button";

export { Button, buttonVariants };
