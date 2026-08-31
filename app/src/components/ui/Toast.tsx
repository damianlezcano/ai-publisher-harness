import type { ReactNode } from "react";

interface ToastProps {
  id: string;
  children: ReactNode;
}

export default function Toast({ id, children }: ToastProps) {
  return (
    <div className="toast" id={id}>
      {children}
    </div>
  );
}
