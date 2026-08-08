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
    primary: "border-rust bg-rust text-white hover:bg-[#874327]",
    secondary: "border-line bg-paper text-ink hover:bg-[#eef3ea]",
    ghost: "border-transparent bg-transparent text-ink hover:bg-black/5",
    danger: "border-[#d4a296] bg-[#fff4f1] text-[#9b2f22] hover:bg-[#ffe7e0]",
  };

  return (
    <button
      className={[
        "inline-flex h-9 items-center justify-center gap-2 rounded-md border px-3 text-sm font-medium transition disabled:cursor-not-allowed disabled:opacity-50",
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
