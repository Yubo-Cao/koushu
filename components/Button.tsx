import type { ButtonHTMLAttributes, ReactNode } from "react";

type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: "primary" | "secondary" | "ghost" | "danger";
  icon?: ReactNode;
};

export function Button({
  variant = "secondary",
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

  return (
    <button
      className={[
        "btn press inline-flex h-9 items-center justify-center gap-2 rounded-pill",
        "text-[13px] font-medium disabled:cursor-not-allowed disabled:opacity-45",
        variants[variant],
        className,
      ].join(" ")}
      {...props}
    >
      {icon}
      {children}
    </button>
  );
}
