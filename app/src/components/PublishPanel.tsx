import { useState } from "react";
import { api, errorMessage } from "../api";
import type { PublicationView } from "../types";
import QrDialog from "./QrDialog";

interface PublishPanelProps {
  projectId: string;
  publication: PublicationView;
  onRefresh: () => void;
}

type Busy = "publishing" | "unpublishing" | null;

export default function PublishPanel({ projectId, publication, onRefresh }: PublishPanelProps) {
  const [busy, setBusy] = useState<Busy>(null);
  const [error, setError] = useState<string | null>(null);
  const [showQr, setShowQr] = useState(false);
  const [copied, setCopied] = useState(false);

  async function publish() {
    setBusy("publishing");
    setError(null);
    try {
      await api.publish(projectId);
      onRefresh();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(null);
    }
  }

  async function unpublish() {
    setBusy("unpublishing");
    setError(null);
    try {
      await api.unpublish(projectId);
      onRefresh();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(null);
    }
  }

  async function copy() {
    if (!publication.publicUrl) return;
    try {
      await navigator.clipboard.writeText(publication.publicUrl);
      setCopied(true);
    } catch {
      setError("No pudimos copiar el enlace.");
    }
  }

  async function open() {
    setError(null);
    try {
      await api.openPublicUrl(projectId);
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  if (publication.state === "published" && publication.publicUrl) {
    return (
      <section className="panel" aria-label="Publicación">
        <h2>Publicación</h2>
        <p className="status published">Publicado</p>
        <p className="url" aria-label="Enlace público">
          {publication.publicUrl}
        </p>
        <div className="row-actions wrap">
          <button type="button" onClick={() => void copy()}>
            {copied ? "Copiado" : "Copiar enlace"}
          </button>
          <button type="button" onClick={() => void open()}>
            Abrir
          </button>
          <button type="button" onClick={() => setShowQr(true)}>
            Mostrar QR
          </button>
          <button
            type="button"
            className="danger"
            disabled={busy === "unpublishing"}
            onClick={() => void unpublish()}
          >
            {busy === "unpublishing" ? "Quitando…" : "Dejar de compartir"}
          </button>
        </div>
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        {showQr && <QrDialog url={publication.publicUrl} onClose={() => setShowQr(false)} />}
      </section>
    );
  }

  return (
    <section className="panel" aria-label="Publicación">
      <h2>Publicación</h2>
      <p className="muted">Este proyecto todavía no se comparte en Internet.</p>
      <button
        type="button"
        className="primary"
        disabled={busy === "publishing"}
        onClick={() => void publish()}
      >
        {busy === "publishing" ? "Publicando…" : "Publicar"}
      </button>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </section>
  );
}
