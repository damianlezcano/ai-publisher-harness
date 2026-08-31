import type { ReactNode } from "react";

export type BadgeTone = "ok" | "neutral" | "warning" | "danger";

interface BadgeProps {
  tone?: BadgeTone;
  children: ReactNode;
}

export default function Badge({ tone = "neutral", children }: BadgeProps) {
  return <span className={`badge ${tone}`}>{children}</span>;
}
