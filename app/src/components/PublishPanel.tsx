import { useState } from "react";
import { api } from "../api";
import type { PublicationView } from "../types";
import QrDialog from "./QrDialog";
import Badge from "./ui/Badge";
import Dialog from "./ui/Dialog";
import EmptyState from "./ui/EmptyState";
import ErrorNotice from "./ui/ErrorNotice";
import { messages } from "../messages";

interface PublishPanelProps {
  projectId: string;
  projectName: string;
  publication: PublicationView;
  onRefresh: () => void;
}

type Busy = "publishing" | "unpublishing" | null;

export default function PublishPanel({
  projectId,
  projectName,
  publication,
  onRefresh,
}: PublishPanelProps) {
  const [busy, setBusy] = useState<Busy>(null);
  const [error, setError] = useState<unknown | null>(null);
  const [copyFailed, setCopyFailed] = useState(false);
  const [stopped, setStopped] = useState(false);
  const [copied, setCopied] = useState(false);
  const [showQr, setShowQr] = useState(false);
  const [confirmStop, setConfirmStop] = useState(false);

  async function publish() {
    setBusy("publishing");
    setError(null);
    setStopped(false);
    try {
      await api.publish(projectId);
      onRefresh();
    } catch (err) {
      setError(err);
    } finally {
      setBusy(null);
    }
  }

  async function unpublish() {
    setConfirmStop(false);
    setBusy("unpublishing");
    setError(null);
    try {
      await api.unpublish(projectId);
      setStopped(true);
      onRefresh();
    } catch (err) {
      setError(err);
    } finally {
      setBusy(null);
    }
  }

  async function copy() {
    if (!publication.publicUrl) return;
    setCopyFailed(false);
    try {
      await navigator.clipboard.writeText(publication.publicUrl);
      setCopied(true);
    } catch {
      setCopyFailed(true);
    }
  }

  async function open() {
    setError(null);
    try {
      await api.openPublicUrl(projectId);
    } catch (err) {
      setError(err);
    }
  }

  const shared = publication.state === "published" && publication.publicUrl !== null;

  return (
    <section className="panel" aria-label={messages.sharing.panelLabel}>
      <h2>{messages.sharing.heading}</h2>

      {shared ? (
        <>
          <Badge tone="ok">{messages.sharing.shared}</Badge>
          <p className="url" aria-label={messages.sharing.linkLabel}>
            {publication.publicUrl}
          </p>
          <div className="row-actions wrap">
            <button type="button" onClick={() => void copy()}>
              {copied ? messages.common.copied : messages.sharing.copyLink}
            </button>
            <button type="button" onClick={() => void open()}>
              {messages.sharing.openLink}
            </button>
            <button type="button" onClick={() => setShowQr(true)}>
              {messages.sharing.showQr}
            </button>
            <button
              type="button"
              className="danger"
              disabled={busy === "unpublishing"}
              onClick={() => setConfirmStop(true)}
            >
              {busy === "unpublishing" ? messages.sharing.stopping : messages.sharing.stopSharing}
            </button>
          </div>
          <p className="notice">{messages.sharing.temporaryNote}</p>
          <p className="notice">{messages.sharing.temporaryGuidance}</p>
        </>
      ) : (
        <>
          {busy === "publishing" ? (
            <>
              <p className="muted">
                <span className="spinner" aria-hidden="true" />
                <span>{messages.sharing.sharing}</span> <span>{messages.sharing.sharingNote}</span>
              </p>
              <button type="button" className="primary" disabled onClick={() => void publish()}>
                {messages.sharing.shareAction}
              </button>
            </>
          ) : (
            <EmptyState
              title={messages.sharing.empty.title}
              actionLabel={messages.sharing.shareAction}
              onAction={() => void publish()}
            />
          )}
          {stopped && <p className="status published">{messages.sharing.stopped}</p>}
        </>
      )}

      {error ? <ErrorNotice error={error} /> : null}
      {copyFailed && (
        <p className="error" role="alert">
          {messages.sharing.copyLinkFailed}
        </p>
      )}

      {showQr && (
        <QrDialog
          projectId={projectId}
          url={publication.publicUrl as string}
          projectName={projectName}
          onClose={() => setShowQr(false)}
        />
      )}

      {confirmStop && (
        <Dialog title={messages.sharing.stopConfirm.title} onClose={() => setConfirmStop(false)}>
          <p>{messages.sharing.stopConfirm.message}</p>
          <div className="dialog-actions">
            <button type="button" className="secondary" onClick={() => setConfirmStop(false)}>
              {messages.common.cancel}
            </button>
            <button type="button" className="danger" onClick={() => void unpublish()}>
              {messages.common.confirm}
            </button>
          </div>
        </Dialog>
      )}
    </section>
  );
}
