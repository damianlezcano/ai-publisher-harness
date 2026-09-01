import { useId } from "react";
import type { ReactNode, RefObject } from "react";
import { messages } from "../../messages";
import { useFocusTrap } from "./useFocusTrap";

interface DialogProps {
  title: string;
  onClose: () => void;
  children: ReactNode;
  initialFocusRef?: RefObject<HTMLElement>;
  className?: string;
  closeButton?: boolean;
}

export default function Dialog({
  title,
  onClose,
  children,
  initialFocusRef,
  className,
  closeButton = false,
}: DialogProps) {
  const titleId = useId();
  const containerRef = useFocusTrap({ active: true, onEscape: onClose, initialFocusRef });

  return (
    <div className="dialog-backdrop" role="presentation" onClick={onClose}>
      <div
        ref={containerRef}
        className={`dialog${className ? ` ${className}` : ""}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby={titleId}
        onClick={(event) => event.stopPropagation()}
      >
        <div className="dialog-header">
          <h2 id={titleId}>{title}</h2>
          {closeButton && (
            <button
              type="button"
              className="ghost close-button"
              aria-label={messages.common.close}
              onClick={onClose}
            >
              <span aria-hidden="true">×</span>
            </button>
          )}
        </div>
        {children}
      </div>
    </div>
  );
}
