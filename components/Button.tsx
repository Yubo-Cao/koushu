import type { ButtonHTMLAttributes, ReactNode } from "react";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "ghost" | "danger";
  /** Height ladder from `--ctl-h*`. `md` is the toolbar/footer default. */
  size?: "sm" | "md" | "lg";
  icon?: ReactNode;
};

export function Button({
  variant = "secondary",
  size = "md",
  icon,
  className = "",
  children,
  ...props
}: ButtonProps) {
  const variants = {
    primary: "btn-primary",
    secondary: "",
    ghost: "btn-ghost",
    danger: "btn-danger",
  };

  const sizes = { sm: "ctl-sm", md: "", lg: "ctl-lg" };

  return (
    <button
      className={[
        // Height, padding, radius, type size and weight all come from `.btn` in
        // globals.css rather than from utilities here. Utilities cannot be
        // overridden by other utilities of the same property, so a base `h-9`
        // made the `size` prop and every caller's `h-8` a no-op.
        "btn press inline-flex items-center justify-center gap-2",
        "disabled:cursor-not-allowed disabled:opacity-45",
        variants[variant],
        sizes[size],
        className,
      ]
        .filter(Boolean)
        .join(" ")}
      {...props}
    >
      {icon}
      {children}
    </button>
  );
}
