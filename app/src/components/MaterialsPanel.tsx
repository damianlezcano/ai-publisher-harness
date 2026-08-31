import { useEffect, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, errorMessage } from "../api";
import { humanDate, humanSize, kindLabel } from "../labels";
import type { MaterialView } from "../types";
import { messages } from "../messages";

interface MaterialsPanelProps {
  projectId: string;
  materials: MaterialView[];
  onRefresh: () => void | Promise<void>;
}

export default function MaterialsPanel({ projectId, materials, onRefresh }: MaterialsPanelProps) {
  const [error, setError] = useState<string | null>(null);
  const [duplicateNote, setDuplicateNote] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [busy, setBusy] = useState(false);
  const [confirmingId, setConfirmingId] = useState<string | null>(null);

  const importRef = useRef<(paths: string[]) => Promise<void>>(async () => {});

  useEffect(() => {
    importRef.current = async (paths: string[]) => {
      if (paths.length === 0) return;
      setBusy(true);
      setError(null);
      setDuplicateNote(null);
      try {
        const report = await api.materialsAddFromPaths(projectId, paths);
        const duplicates = report.items.filter((item) => item.status === "duplicate");
        const failures = report.items.filter(
          (item) => item.status === "unsupported" || item.status === "failed",
        );
        if (duplicates.length > 0) {
          setDuplicateNote(messages.material.duplicateSingle);
        }
        if (failures.length > 0) {
          setError(messages.material.importPartialFailure);
        }
        await onRefresh();
      } catch (err) {
        setError(errorMessage(err));
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
    setDuplicateNote(null);
    try {
      const path = await api.pickFile();
      if (path) {
        setBusy(true);
        try {
          await api.materialAddFromPath(projectId, path);
          await onRefresh();
        } catch (err) {
          setError(errorMessage(err));
        } finally {
          setBusy(false);
        }
      }
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  async function openMaterial(materialId: string) {
    setError(null);
    try {
      await api.materialOpen(projectId, materialId);
    } catch (err) {
      setError(errorMessage(err));
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
      setError(errorMessage(err));
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
      <button type="button" className="secondary" onClick={() => void pick()} disabled={busy}>
        {messages.material.addFile}
      </button>
      {duplicateNote && <p className="notice">{duplicateNote}</p>}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      {materials.length === 0 ? (
        <p className="muted">{messages.material.empty.title}</p>
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
