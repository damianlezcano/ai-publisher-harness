import { useState } from "react";
import { api } from "../api";
import { kindIcon, kindLabel } from "../labels";
import type { CreationView, PreviewData } from "../types";
import PreviewModal from "./PreviewModal";
import { messages } from "../messages";
import EmptyState from "./ui/EmptyState";
import ErrorNotice from "./ui/ErrorNotice";

export interface CreationCardShareProps {
  onShare: (creationId: string) => void;
  shared: boolean;
  busy: boolean;
}

interface CreationsPanelProps {
  projectId: string;
  creations: CreationView[];
  onRefresh: () => void | Promise<void>;
  share?: CreationCardShareProps;
}

interface CreationCardProps {
  projectId: string;
  creation: CreationView;
  onRefresh: () => void | Promise<void>;
  share?: CreationCardShareProps;
}

function supportsInAppPreview(kind: string): boolean {
  return kind === "image" || kind === "file";
}

function isWebKind(kind: string): boolean {
  return kind === "web";
}

export function CreationCard(props: CreationCardProps) {
  const { projectId, creation, share } = props;
  const [error, setError] = useState<unknown>(null);
  const [previewAnnouncement, setPreviewAnnouncement] = useState<string | null>(null);
  const [preview, setPreview] = useState<{ creation: CreationView; data: PreviewData } | null>(
    null,
  );

  async function open() {
    setError(null);
    setPreviewAnnouncement(null);
    try {
      if (isWebKind(creation.kind)) {
        setPreviewAnnouncement(messages.creation.previewLoading);
        await api.previewOpenWeb(projectId, creation.id);
        return;
      }
      if (supportsInAppPreview(creation.kind)) {
        try {
          const data = await api.previewData(projectId, "creation", creation.id);
          setPreview({ creation, data });
          return;
        } catch {
          // Fall through to the system opener when in-app viewing is not available.
        }
      }
      await api.creationOpen(projectId, creation.id);
    } catch (err) {
      setPreviewAnnouncement(null);
      setError(err);
    }
  }

  return (
    <div className="item-row creation-card">
      <p className="sr-only" aria-live="polite">
        {previewAnnouncement ?? ""}
      </p>
      {error != null ? <ErrorNotice error={error} /> : null}
      <div className="creation-card-main">
        <span className="creation-card-title">
          <span className="creation-card-icon" aria-hidden="true">
            {kindIcon(creation.kind)}
          </span>
          <span className="sr-only">{creation.displayName}</span>
          <span className="item-name">{kindLabel(creation.kind)}</span>
        </span>
      </div>
      <span className="row-actions wrap">
        <button
          type="button"
          className="primary"
          aria-label={`${messages.common.open}: ${creation.displayName}`}
          onClick={() => void open()}
        >
          {messages.common.open}
        </button>
        {share != null && (
          <button
            type="button"
            className="secondary"
            disabled={share.busy || share.shared}
            aria-label={`${share.shared ? messages.sharing.shared : share.busy ? messages.sharing.sharing : messages.sharing.shareAction}: ${creation.displayName}`}
            onClick={() => void share.onShare(creation.id)}
          >
            {share.shared
              ? messages.sharing.shared
              : share.busy
                ? messages.sharing.sharing
                : messages.sharing.shareAction}
          </button>
        )}
      </span>
      {preview && (
        <PreviewModal
          title={preview.creation.displayName}
          preview={preview.data}
          meta={{
            name: preview.creation.displayName,
            byteSize: preview.creation.byteSize,
            kind: preview.creation.kind,
          }}
          onClose={() => setPreview(null)}
          onOpenExternal={() => {
            setPreview(null);
            void api.creationOpen(projectId, creation.id);
          }}
        />
      )}
    </div>
  );
}

export default function CreationsPanel({
  projectId,
  creations,
  onRefresh,
  share,
}: CreationsPanelProps) {
  return (
    <section className="panel" aria-label={messages.creation.panelLabel}>
      <h2>{messages.creation.heading}</h2>
      {creations.length === 0 ? (
        <EmptyState title={messages.creation.empty.title} body={messages.creation.empty.hint} />
      ) : (
        <ul className="item-list">
          {creations.map((creation) => (
            <li key={creation.id}>
              <CreationCard
                projectId={projectId}
                creation={creation}
                onRefresh={onRefresh}
                share={share}
              />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
