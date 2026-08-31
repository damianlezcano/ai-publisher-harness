import { useState } from "react";
import { api } from "../api";
import { humanDate, humanSize, kindLabel } from "../labels";
import type { CreationView, PreviewData } from "../types";
import PreviewModal from "./PreviewModal";
import { messages } from "../messages";
import EmptyState from "./ui/EmptyState";
import ErrorNotice from "./ui/ErrorNotice";

interface CreationsPanelProps {
  projectId: string;
  creations: CreationView[];
  onRefresh: () => void | Promise<void>;
}

interface CreationCardProps {
  projectId: string;
  creation: CreationView;
  onRefresh: () => void | Promise<void>;
}

function supportsInAppPreview(kind: string): boolean {
  return kind === "image" || kind === "file";
}

function isWebKind(kind: string): boolean {
  return kind === "web";
}

export function CreationCard(props: CreationCardProps) {
  const { projectId, creation } = props;
  const [error, setError] = useState<unknown>(null);
  const [previewAnnouncement, setPreviewAnnouncement] = useState<string | null>(null);
  const [preview, setPreview] = useState<{ creation: CreationView; data: PreviewData } | null>(
    null,
  );

  async function open() {
    setError(null);
    try {
      await api.creationOpen(projectId, creation.id);
    } catch (err) {
      setError(err);
    }
  }

  async function showPreview() {
    setError(null);
    setPreviewAnnouncement(null);
    if (isWebKind(creation.kind)) {
      setPreviewAnnouncement(messages.creation.previewLoading);
      try {
        await api.previewOpenWeb(projectId, creation.id);
      } catch (err) {
        setPreviewAnnouncement(null);
        setError(err);
      }
      return;
    }
    if (!supportsInAppPreview(creation.kind)) return;
    try {
      const data = await api.previewData(projectId, "creation", creation.id);
      setPreview({ creation, data });
    } catch (err) {
      setError(err);
    }
  }

  return (
    <div className="item-row creation-card">
      <p className="sr-only" aria-live="polite">
        {previewAnnouncement ?? ""}
      </p>
      {error != null ? <ErrorNotice error={error} /> : null}
      <span className="item-name">{creation.displayName}</span>
      <span className="item-meta">
        {kindLabel(creation.kind)} · {humanSize(creation.byteSize)} ·{" "}
        {humanDate(creation.createdAt)}
      </span>
      <span className="row-actions wrap">
        {(supportsInAppPreview(creation.kind) || isWebKind(creation.kind)) && (
          <button type="button" className="secondary" onClick={() => void showPreview()}>
            {messages.creation.preview}
          </button>
        )}
        <button type="button" className="primary" onClick={() => void open()}>
          {isWebKind(creation.kind) ? messages.creation.openInBrowser : messages.common.open}
        </button>
      </span>
      {preview && (
        <PreviewModal
          title={preview.creation.displayName}
          preview={preview.data}
          onClose={() => setPreview(null)}
        />
      )}
    </div>
  );
}

export default function CreationsPanel({ projectId, creations, onRefresh }: CreationsPanelProps) {
  return (
    <section className="panel" aria-label={messages.creation.panelLabel}>
      <h2>{messages.creation.heading}</h2>
      {creations.length === 0 ? (
        <EmptyState title={messages.creation.empty.title} body={messages.creation.empty.hint} />
      ) : (
        <ul className="item-list">
          {creations.map((creation) => (
            <li key={creation.id}>
              <CreationCard projectId={projectId} creation={creation} onRefresh={onRefresh} />
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
