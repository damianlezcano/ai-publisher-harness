import { useState } from "react";
import { api, errorMessage } from "../api";
import { humanDate, humanSize, kindLabel, visibilityLabel } from "../labels";
import type { CreationView, PreviewData } from "../types";
import PreviewModal from "./PreviewModal";
import { messages } from "../messages";

interface CreationsPanelProps {
  projectId: string;
  creations: CreationView[];
  onRefresh: () => void | Promise<void>;
}

function supportsInAppPreview(kind: string): boolean {
  return kind === "image" || kind === "file";
}

function isWebKind(kind: string): boolean {
  return kind === "web";
}

export default function CreationsPanel({ projectId, creations, onRefresh }: CreationsPanelProps) {
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [previewAnnouncement, setPreviewAnnouncement] = useState<string | null>(null);
  const [preview, setPreview] = useState<{ creation: CreationView; data: PreviewData } | null>(
    null,
  );

  async function toggle(creation: CreationView) {
    setBusyId(creation.id);
    setError(null);
    try {
      await api.creationSetVisibility(projectId, creation.id, creation.visibility !== "public");
      await onRefresh();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusyId(null);
    }
  }

  async function open(creationId: string) {
    setError(null);
    try {
      await api.creationOpen(projectId, creationId);
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  async function showPreview(creation: CreationView) {
    setError(null);
    setPreviewAnnouncement(null);
    if (isWebKind(creation.kind)) {
      setPreviewAnnouncement(messages.creation.previewLoading);
      try {
        await api.previewOpenWeb(projectId, creation.id);
      } catch (err) {
        setPreviewAnnouncement(null);
        setError(errorMessage(err));
      }
      return;
    }
    if (!supportsInAppPreview(creation.kind)) return;
    setBusyId(creation.id);
    try {
      const data = await api.previewData(projectId, "creation", creation.id);
      setPreview({ creation, data });
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusyId(null);
    }
  }

  return (
    <section className="panel" aria-label={messages.creation.panelLabel}>
      <h2>{messages.creation.heading}</h2>
      <p className="sr-only" aria-live="polite">
        {previewAnnouncement ?? ""}
      </p>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      {creations.length === 0 ? (
        <p className="muted">{messages.creation.empty.title}</p>
      ) : (
        <ul className="item-list">
          {creations.map((creation) => (
            <li key={creation.id} className="item-row creation-card">
              <span className="item-name">{creation.displayName}</span>
              <span className="item-meta">
                {kindLabel(creation.kind)} · {visibilityLabel(creation.visibility)} ·{" "}
                {humanSize(creation.byteSize)} · {humanDate(creation.createdAt)}
              </span>
              <span className="row-actions wrap">
                {(supportsInAppPreview(creation.kind) || isWebKind(creation.kind)) && (
                  <button
                    type="button"
                    className="secondary"
                    disabled={busyId === creation.id}
                    onClick={() => void showPreview(creation)}
                  >
                    {messages.creation.preview}
                  </button>
                )}
                <button
                  type="button"
                  className="secondary"
                  disabled={busyId === creation.id}
                  onClick={() => void toggle(creation)}
                >
                  {visibilityLabel(creation.visibility)}
                </button>
                <button
                  type="button"
                  className="primary"
                  disabled={busyId === creation.id}
                  onClick={() => void open(creation.id)}
                >
                  {isWebKind(creation.kind)
                    ? messages.creation.openInBrowser
                    : messages.common.open}
                </button>
              </span>
            </li>
          ))}
        </ul>
      )}
      {preview && (
        <PreviewModal
          title={preview.creation.displayName}
          preview={preview.data}
          onClose={() => setPreview(null)}
        />
      )}
    </section>
  );
}
