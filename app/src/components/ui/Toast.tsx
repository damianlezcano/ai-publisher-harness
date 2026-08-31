import type { ReactNode } from "react";

interface ToastProps {
  id: string;
  children: ReactNode;
}

export default function Toast({ children }: ToastProps) {
  return <div className="toast">{children}</div>;
}
