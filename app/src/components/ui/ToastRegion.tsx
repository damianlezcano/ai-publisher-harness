import Toast from "./Toast";
import type { ToastItem } from "./useToast";

interface ToastRegionProps {
  toasts: ToastItem[];
}

export default function ToastRegion({ toasts }: ToastRegionProps) {
  return (
    <div role="status" aria-live="polite" aria-atomic="true">
      <div className="toast-container">
        {toasts.map((toast) => (
          <Toast key={toast.id} id={toast.id}>
            {toast.children}
          </Toast>
        ))}
      </div>
    </div>
  );
}
