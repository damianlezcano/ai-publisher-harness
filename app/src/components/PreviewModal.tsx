import type { PreviewData } from "../types";
import { messages } from "../messages";
import Dialog from "./ui/Dialog";

interface PreviewModalProps {
  title: string;
  preview: PreviewData;
  onClose: () => void;
}

function isImageContentType(contentType: string): boolean {
  return contentType.startsWith("image/");
}

export default function PreviewModal({ title, preview, onClose }: PreviewModalProps) {
  const image = isImageContentType(preview.contentType);
  let textContent: string | null = null;
  if (!image) {
    const binary = atob(preview.dataBase64);
    const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
    textContent = new TextDecoder().decode(bytes);
  }

  return (
    <Dialog title={title} onClose={onClose} className="preview-modal">
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
        <button type="button" className="secondary" onClick={onClose}>
          {messages.common.close}
        </button>
      </div>
    </Dialog>
  );
}
