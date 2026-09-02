import type { PreviewData } from "../types";
import { humanSize, kindLabel, messages } from "../messages";
import Dialog from "./ui/Dialog";

interface PreviewMeta {
  name: string;
  byteSize: number;
  kind: string;
}

interface PreviewModalProps {
  title: string;
  preview: PreviewData;
  meta?: PreviewMeta;
  onClose: () => void;
  onOpenExternal?: () => void | Promise<void>;
}

const TEXT_LIKE_TYPES = new Set([
  "application/json",
  "application/xml",
  "application/yaml",
  "application/x-yaml",
  "text/yaml",
  "application/javascript",
  "application/x-javascript",
]);

function isTextContentType(contentType: string): boolean {
  const ct = contentType.trim().toLowerCase();
  return ct.startsWith("text/") || TEXT_LIKE_TYPES.has(ct);
}

function decodeBase64(dataBase64: string): Uint8Array {
  const binary = atob(dataBase64);
  const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
  return bytes;
}

function sniffImageContentType(bytes: Uint8Array): string | null {
  if (
    bytes.length >= 8 &&
    bytes[0] === 0x89 &&
    bytes[1] === 0x50 &&
    bytes[2] === 0x4e &&
    bytes[3] === 0x47 &&
    bytes[4] === 0x0d &&
    bytes[5] === 0x0a &&
    bytes[6] === 0x1a &&
    bytes[7] === 0x0a
  ) {
    return "image/png";
  }
  if (bytes.length >= 3 && bytes[0] === 0xff && bytes[1] === 0xd8 && bytes[2] === 0xff) {
    return "image/jpeg";
  }
  if (
    bytes.length >= 6 &&
    bytes[0] === 0x47 &&
    bytes[1] === 0x49 &&
    bytes[2] === 0x46 &&
    bytes[3] === 0x38
  ) {
    return "image/gif";
  }
  if (
    bytes.length >= 12 &&
    bytes[0] === 0x52 &&
    bytes[1] === 0x49 &&
    bytes[2] === 0x46 &&
    bytes[3] === 0x46 &&
    bytes[8] === 0x57 &&
    bytes[9] === 0x45 &&
    bytes[10] === 0x42 &&
    bytes[11] === 0x50
  ) {
    return "image/webp";
  }
  return null;
}

export default function PreviewModal({
  title,
  preview,
  meta,
  onClose,
  onOpenExternal,
}: PreviewModalProps) {
  const bytes = decodeBase64(preview.dataBase64);
  const sniffed = sniffImageContentType(bytes);
  const isImage = preview.contentType.startsWith("image/") || sniffed !== null;
  const resolvedImageType = sniffed ?? preview.contentType;
  let textContent: string | null = null;
  if (!isImage && isTextContentType(preview.contentType)) {
    textContent = new TextDecoder().decode(bytes);
  }

  return (
    <Dialog title={title} onClose={onClose} className="preview-modal">
      {isImage ? (
        <img
          className="preview-image"
          src={`data:${resolvedImageType};base64,${preview.dataBase64}`}
          alt={title}
        />
      ) : textContent !== null ? (
        <pre className="preview-text">{textContent}</pre>
      ) : (
        <div className="preview-binary">
          <p className="preview-binary-title">{meta?.name ?? title}</p>
          {meta && (
            <p className="preview-binary-meta">
              {kindLabel(meta.kind)} · {humanSize(meta.byteSize)}
            </p>
          )}
          <p className="muted">{messages.preview.binaryHint}</p>
          {onOpenExternal && (
            <div className="preview-actions">
              <button type="button" className="secondary" onClick={() => void onOpenExternal()}>
                {messages.preview.openExternal}
              </button>
            </div>
          )}
        </div>
      )}
      <div className="preview-actions">
        <button type="button" className="secondary" onClick={onClose}>
          {messages.common.close}
        </button>
      </div>
    </Dialog>
  );
}
