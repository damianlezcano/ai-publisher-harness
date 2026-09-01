import { useEffect, useRef } from "react";
import QrDialog from "./QrDialog";
import Dialog from "./ui/Dialog";
import ErrorNotice from "./ui/ErrorNotice";
import { messages } from "../messages";
import {
  useShareControl,
  type ShareControlState,
  type UseShareControlInput,
} from "./useShareControl";

export type ShareControlProps = UseShareControlInput & {
  projectName: string;
  share?: ShareControlState;
};

export default function ShareControl({
  projectId,
  projectName,
  publication,
  onRefresh,
  share: externalShare,
}: ShareControlProps) {
  const internalShare = useShareControl({ projectId, publication, onRefresh });
  const {
    busy,
    error,
    copyFailed,
    copied,
    showQr,
    setShowQr,
    confirmStop,
    setConfirmStop,
    menuOpen,
    setMenuOpen,
    shared,
    publish,
    unpublish,
    copy,
    open,
  } = externalShare ?? internalShare;
  const controlRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menuOpen) return;
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") setMenuOpen(false);
    }
    function handleClick(event: MouseEvent) {
      if (controlRef.current && !controlRef.current.contains(event.target as Node)) {
        setMenuOpen(false);
      }
    }
    document.addEventListener("keydown", handleKeyDown);
    document.addEventListener("mousedown", handleClick);
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
      document.removeEventListener("mousedown", handleClick);
    };
  }, [menuOpen, setMenuOpen]);

  const triggerLabel = shared
    ? busy === "unpublishing"
      ? messages.sharing.stopping
      : messages.sharing.shared
    : busy === "publishing"
      ? messages.sharing.sharing
      : messages.sharing.shareAction;

  return (
    <div className="share-control" ref={controlRef}>
      {shared ? (
        <>
          <button
            type="button"
            className="secondary share-control-trigger"
            aria-expanded={menuOpen}
            aria-haspopup="menu"
            disabled={busy === "unpublishing"}
            onClick={() => setMenuOpen((isOpen) => !isOpen)}
          >
            {triggerLabel}
            <span aria-hidden="true"> ▼</span>
          </button>
          {menuOpen && (
            <div
              className="share-control-menu"
              role="menu"
              aria-label={messages.sharing.panelLabel}
            >
              <button type="button" role="menuitem" onClick={() => void copy()}>
                {copied ? messages.common.copied : messages.sharing.copyLink}
              </button>
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  setMenuOpen(false);
                  void open();
                }}
              >
                {messages.sharing.openLink}
              </button>
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  setMenuOpen(false);
                  setShowQr(true);
                }}
              >
                {messages.sharing.showQr}
              </button>
              <button
                type="button"
                role="menuitem"
                className="danger"
                onClick={() => {
                  setMenuOpen(false);
                  setConfirmStop(true);
                }}
              >
                {messages.sharing.stopSharing}
              </button>
              <p className="share-control-hint">{messages.sharing.temporaryNote}</p>
            </div>
          )}
        </>
      ) : (
        <button
          type="button"
          className="secondary share-control-trigger"
          disabled={busy === "publishing"}
          onClick={() => void publish()}
        >
          {triggerLabel}
        </button>
      )}

      {error ? <ErrorNotice error={error} /> : null}
      {copyFailed && (
        <p className="error" role="alert">
          {messages.sharing.copyLinkFailed}
        </p>
      )}

      {showQr && publication.publicUrl && (
        <QrDialog
          projectId={projectId}
          url={publication.publicUrl}
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
    </div>
  );
}
