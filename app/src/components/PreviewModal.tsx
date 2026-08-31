import { useEffect, useRef } from "react";
import type { PreviewData } from "../types";

interface PreviewModalProps {
  title: string;
  preview: PreviewData;
  onClose: () => void;
}

function isImageContentType(contentType: string): boolean {
  return contentType.startsWith("image/");
}

export default function PreviewModal({ title, preview, onClose }: PreviewModalProps) {
  const modalRef = useRef<HTMLDivElement>(null);
  const closeRef = useRef<HTMLButtonElement>(null);
  const triggerRef = useRef<HTMLElement | null>(null);

  useEffect(() => {
    triggerRef.current = document.activeElement as HTMLElement | null;
    closeRef.current?.focus();

    function onKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
        return;
      }
      if (event.key !== "Tab" || !modalRef.current) return;
      const focusable = modalRef.current.querySelectorAll<HTMLElement>(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
      );
      if (focusable.length === 0) return;
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (event.shiftKey && document.activeElement === first) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    }

    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      triggerRef.current?.focus();
    };
  }, [onClose]);

  const image = isImageContentType(preview.contentType);
  let textContent: string | null = null;
  if (!image) {
    const binary = atob(preview.dataBase64);
    const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
    textContent = new TextDecoder().decode(bytes);
  }

  return (
    <div className="preview-backdrop" role="presentation" onClick={onClose}>
      <div
        ref={modalRef}
        className="preview-modal"
        role="dialog"
        aria-modal="true"
        aria-label={`Vista previa: ${title}`}
        onClick={(e) => e.stopPropagation()}
      >
        <h2 className="preview-title">{title}</h2>
        {image ? (
          <img
            className="preview-image"
            src={`data:${preview.contentType};base64,${preview.dataBase64}`}
            alt={title}
          />
        ) : (
          <pre className="preview-text">{textContent}</pre>
        )}
        <div className="preview-actions">
          <button ref={closeRef} type="button" className="secondary" onClick={onClose}>
            Cerrar
          </button>
        </div>
      </div>
    </div>
  );
}
