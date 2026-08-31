import { useState } from "react";
import { api } from "../api";
import { humanDate, humanSize, kindLabel } from "../labels";
import type { CreationView, PreviewData } from "../types";
import PreviewModal from "./PreviewModal";
import { messages } from "../messages";
import EmptyState from "./ui/EmptyState";
import Badge from "./ui/Badge";
import ErrorNotice from "./ui/ErrorNotice";

interface CreationsPanelProps {
  projectId: string;
  creations: CreationView[];
  onRefresh: () => void | Promise<void>;
  shared?: boolean;
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

function isPublicVisibility(visibility: string): boolean {
  return visibility === "public";
}

function visibilityStateLabel(visibility: string): string {
  return isPublicVisibility(visibility)
    ? messages.creation.visibilityPublic
    : messages.creation.visibilityPrivate;
}

export function CreationCard({ projectId, creation, onRefresh }: CreationCardProps) {
  const [error, setError] = useState<unknown>(null);
  const [busy, setBusy] = useState(false);
  const [previewAnnouncement, setPreviewAnnouncement] = useState<string | null>(null);
  const [preview, setPreview] = useState<{ creation: CreationView; data: PreviewData } | null>(
    null,
  );

  async function toggle() {
    setBusy(true);
    setError(null);
    try {
      await api.creationSetVisibility(projectId, creation.id, creation.visibility !== "public");
      await onRefresh();
    } catch (err) {
      setError(err);
    } finally {
      setBusy(false);
    }
  }

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
    setBusy(true);
    try {
      const data = await api.previewData(projectId, "creation", creation.id);
      setPreview({ creation, data });
    } catch (err) {
      setError(err);
    } finally {
      setBusy(false);
    }
  }

  const isPublic = isPublicVisibility(creation.visibility);
  const stateLabel = visibilityStateLabel(creation.visibility);

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
          <button
            type="button"
            className="secondary"
            disabled={busy}
            onClick={() => void showPreview()}
          >
            {messages.creation.preview}
          </button>
        )}
        <Badge tone={isPublic ? "ok" : "neutral"}>{stateLabel}</Badge>
        <button
          type="button"
          role="switch"
          aria-checked={isPublic}
          className="secondary"
          disabled={busy}
          onClick={() => void toggle()}
        >
          {stateLabel}
        </button>
        <button type="button" className="primary" disabled={busy} onClick={() => void open()}>
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

export default function CreationsPanel({
  projectId,
  creations,
  onRefresh,
  shared = false,
}: CreationsPanelProps) {
  return (
    <section className="panel" aria-label={messages.creation.panelLabel}>
      <h2>{messages.creation.heading}</h2>
      {shared && <p className="muted">{messages.creation.sharedClarifier}</p>}
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
