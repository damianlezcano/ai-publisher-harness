import { useEffect, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api } from "../api";
import { humanDate, humanSize, kindLabel } from "../labels";
import { messages } from "../messages";
import type { MaterialImportResult, MaterialView, MaterialsImportReport } from "../types";
import EmptyState from "./ui/EmptyState";
import ErrorNotice from "./ui/ErrorNotice";

interface MaterialsPanelProps {
  projectId: string;
  materials: MaterialView[];
  onRefresh: () => void | Promise<void>;
}

function importDetailLabel(item: MaterialImportResult): string {
  switch (item.status) {
    case "added":
      return messages.material.perFileAdded(item.sourceName);
    case "duplicate":
      return messages.material.perFileDuplicate(item.sourceName);
    default:
      return messages.material.perFileFailed(item.sourceName);
  }
}

export default function MaterialsPanel({ projectId, materials, onRefresh }: MaterialsPanelProps) {
  const [error, setError] = useState<unknown | null>(null);
  const [importSummary, setImportSummary] = useState<string | null>(null);
  const [importDetails, setImportDetails] = useState<MaterialImportResult[] | null>(null);
  const [dragging, setDragging] = useState(false);
  const [busy, setBusy] = useState(false);
  const [confirmingId, setConfirmingId] = useState<string | null>(null);

  const importRef = useRef<(paths: string[]) => Promise<void>>(async () => {});

  useEffect(() => {
    importRef.current = async (paths: string[]) => {
      if (paths.length === 0) return;
      setBusy(true);
      setError(null);
      setImportSummary(null);
      setImportDetails(null);
      try {
        const report: MaterialsImportReport = await api.materialsAddFromPaths(projectId, paths);
        const added = report.items.filter((item) => item.status === "added").length;
        const duplicate = report.items.filter((item) => item.status === "duplicate").length;
        const failed = report.items.filter(
          (item) => item.status === "unsupported" || item.status === "failed",
        ).length;
        setImportSummary(messages.material.importSummary(added, duplicate, failed));
        if (duplicate > 0 || failed > 0) {
          setImportDetails(report.items);
        }
        await onRefresh();
      } catch (err) {
        setError(err);
      } finally {
        setBusy(false);
      }
    };
  }, [projectId, onRefresh]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let active = true;
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        if (!active) return;
        if (event.payload.type === "over") {
          setDragging(true);
        } else if (event.payload.type === "drop") {
          setDragging(false);
          void importRef.current(event.payload.paths);
        } else if (event.payload.type === "leave") {
          setDragging(false);
        }
      })
      .then((fn) => {
        if (active) unlisten = fn;
      });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  async function pick() {
    setError(null);
    setImportSummary(null);
    setImportDetails(null);
    try {
      const path = await api.pickFile();
      if (path) {
        setBusy(true);
        try {
          await api.materialAddFromPath(projectId, path);
          await onRefresh();
        } catch (err) {
          setError(err);
        } finally {
          setBusy(false);
        }
      }
    } catch (err) {
      setError(err);
    }
  }

  async function openMaterial(materialId: string) {
    setError(null);
    try {
      await api.materialOpen(projectId, materialId);
    } catch (err) {
      setError(err);
    }
  }

  async function removeMaterial(material: MaterialView) {
    setBusy(true);
    setError(null);
    try {
      await api.materialRemove(projectId, material.id);
      setConfirmingId(null);
      await onRefresh();
    } catch (err) {
      setError(err);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section
      className="panel"
      aria-label={messages.material.panelLabel}
      data-dragging={dragging || undefined}
    >
      <h2>{messages.material.heading}</h2>
      {(busy || materials.length > 0) && (
        <div className="row-actions wrap">
          {busy && (
            <span className="notice" role="status">
              <span className="spinner" aria-hidden="true" />
              {messages.progress.importing}
            </span>
          )}
          {materials.length > 0 && (
            <button type="button" className="secondary" onClick={() => void pick()} disabled={busy}>
              {messages.material.addFile}
            </button>
          )}
        </div>
      )}
      {error !== null && <ErrorNotice error={error} />}
      {importSummary && <p className="notice">{importSummary}</p>}
      {importDetails && importDetails.length > 0 && (
        <ul className="chip-list">
          {importDetails.map((item) => (
            <li key={item.sourceName} className="chip">
              {importDetailLabel(item)}
            </li>
          ))}
        </ul>
      )}
      {materials.length === 0 ? (
        <EmptyState
          title={messages.material.empty.title}
          body={messages.material.empty.pasteHint}
          actionLabel={busy ? undefined : messages.material.addFile}
          onAction={() => void pick()}
        />
      ) : (
        <ul className="material-list">
          {materials.map((material) => (
            <li key={material.id} className="material-card">
              <div className="material-card-body">
                <span className="item-name">{material.displayName}</span>
                <span className="item-meta">
                  {kindLabel(material.kind)} · {humanSize(material.byteSize)} ·{" "}
                  {humanDate(material.createdAt)}
                </span>
              </div>
              {confirmingId === material.id ? (
                <div
                  className="remove-confirm"
                  role="group"
                  aria-label={messages.material.removeConfirmAriaLabel}
                >
                  <p>{messages.material.removeConfirm(material.displayName)}</p>
                  <div className="row-actions wrap">
                    <button
                      type="button"
                      className="secondary"
                      disabled={busy}
                      onClick={() => setConfirmingId(null)}
                    >
                      {messages.common.cancel}
                    </button>
                    <button
                      type="button"
                      className="danger"
                      disabled={busy}
                      onClick={() => void removeMaterial(material)}
                    >
                      {messages.common.remove}
                    </button>
                  </div>
                </div>
              ) : (
                <div className="row-actions wrap">
                  <button
                    type="button"
                    className="secondary"
                    disabled={busy}
                    onClick={() => void openMaterial(material.id)}
                  >
                    {messages.common.open}
                  </button>
                  <button
                    type="button"
                    className="danger"
                    disabled={busy}
                    onClick={() => setConfirmingId(material.id)}
                  >
                    {messages.common.remove}
                  </button>
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
