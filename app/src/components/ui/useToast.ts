import { useCallback, useState } from "react";
import type { ReactNode } from "react";

export interface ToastItem {
  id: string;
  children: ReactNode;
}

let toastCounter = 0;

export function useToast() {
  const [toasts, setToasts] = useState<ToastItem[]>([]);

  const show = useCallback((message: string) => {
    const id = `toast-${toastCounter++}`;
    setToasts((previous) => [...previous, { id, children: message }]);
  }, []);

  const dismiss = useCallback((id: string) => {
    setToasts((previous) => previous.filter((toast) => toast.id !== id));
  }, []);

  return { toasts, show, dismiss };
}
