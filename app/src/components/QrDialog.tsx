import { useEffect, useState } from "react";
import QRCode from "qrcode";
import Dialog from "./ui/Dialog";
import ErrorNotice from "./ui/ErrorNotice";
import { api } from "../api";
import { messages } from "../messages";

interface QrDialogProps {
  projectId: string;
  url: string;
  projectName: string;
  onClose: () => void;
}

export default function QrDialog({ projectId, url, projectName, onClose }: QrDialogProps) {
  const [dataUrl, setDataUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [copyFailed, setCopyFailed] = useState(false);
  const [openError, setOpenError] = useState<unknown | null>(null);

  useEffect(() => {
    let active = true;
    void QRCode.toDataURL(url, { width: 360, margin: 1 })
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

  async function copy() {
    setCopyFailed(false);
    try {
      await navigator.clipboard.writeText(url);
      setCopied(true);
    } catch {
      setCopyFailed(true);
    }
  }

  async function open() {
    setOpenError(null);
    try {
      await api.openPublicUrl(projectId);
    } catch (err) {
      setOpenError(err);
    }
  }

  return (
    <Dialog title={projectName} onClose={onClose}>
      {error ? (
        <p className="error" role="alert">
          {error}
        </p>
      ) : dataUrl ? (
        <img src={dataUrl} alt={messages.qr.altForProject(projectName, url)} className="qr" />
      ) : (
        <p className="muted">{messages.qr.generating}</p>
      )}
      <div className="row-actions wrap">
        <button type="button" onClick={() => void copy()}>
          {copied ? messages.common.copied : messages.sharing.copyLink}
        </button>
        <button type="button" onClick={() => void open()}>
          {messages.sharing.openLink}
        </button>
      </div>
      {copyFailed && (
        <p className="error" role="alert">
          {messages.sharing.copyLinkFailed}
        </p>
      )}
      {openError ? <ErrorNotice error={openError} /> : null}
      <div className="dialog-actions">
        <button type="button" className="secondary" onClick={onClose}>
          {messages.common.close}
        </button>
      </div>
    </Dialog>
  );
}
