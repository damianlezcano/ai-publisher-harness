import { useId } from "react";
import type { ReactNode, RefObject } from "react";
import { useFocusTrap } from "./useFocusTrap";

interface DialogProps {
  title: string;
  onClose: () => void;
  children: ReactNode;
  initialFocusRef?: RefObject<HTMLElement>;
  className?: string;
}

export default function Dialog({
  title,
  onClose,
  children,
  initialFocusRef,
  className,
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
        <h2 id={titleId}>{title}</h2>
        {children}
      </div>
    </div>
  );
}
