import { useEffect, useState } from "react";
import QRCode from "qrcode";
import { messages } from "../messages";

interface QrDialogProps {
  url: string;
  onClose: () => void;
}

export default function QrDialog({ url, onClose }: QrDialogProps) {
  const [dataUrl, setDataUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let active = true;
    void QRCode.toDataURL(url, { width: 300, margin: 1 })
      .then((value) => {
        if (active) setDataUrl(value);
      })
      .catch(() => {
        if (active) setError(messages.qr.generateFailed);
      });
    return () => {
      active = false;
    };
  }, [url]);

  return (
    <div className="dialog-backdrop" role="presentation">
      <div className="dialog" role="dialog" aria-modal="true" aria-labelledby="qr-title">
        <h2 id="qr-title">{messages.qr.title}</h2>
        {error ? (
          <p className="error" role="alert">
            {error}
          </p>
        ) : dataUrl ? (
          <img src={dataUrl} alt={messages.qr.altForUrl(url)} className="qr" />
        ) : (
          <p className="muted">{messages.qr.generating}</p>
        )}
        <div className="dialog-actions">
          <button type="button" className="secondary" onClick={onClose}>
            {messages.common.close}
          </button>
        </div>
      </div>
    </div>
  );
}
