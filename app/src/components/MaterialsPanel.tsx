import { useEffect, useRef, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, errorMessage } from "../api";
import { humanSize, kindLabel } from "../labels";
import type { MaterialView } from "../types";

interface MaterialsPanelProps {
  projectId: string;
  materials: MaterialView[];
  onRefresh: () => void;
}

export default function MaterialsPanel({ projectId, materials, onRefresh }: MaterialsPanelProps) {
  const [error, setError] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const [busy, setBusy] = useState(false);

  const addRef = useRef<(path: string) => Promise<void>>(async () => {});

  useEffect(() => {
    addRef.current = async (path: string) => {
      setBusy(true);
      setError(null);
      try {
        await api.materialAddFromPath(projectId, path);
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
          for (const path of event.payload.paths) {
            void addRef.current(path);
          }
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
    try {
      const path = await api.pickFile();
      if (path) await addRef.current(path);
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  return (
    <section className="panel" aria-label="Materiales" data-dragging={dragging || undefined}>
      <h2>Materiales</h2>
      <button type="button" className="secondary" onClick={() => void pick()} disabled={busy}>
        Agregar archivo
      </button>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      {materials.length === 0 ? (
        <p className="muted">Arrastrá archivos acá o usá “Agregar archivo”.</p>
      ) : (
        <ul className="item-list">
          {materials.map((material) => (
            <li key={material.id} className="item-row">
              <span className="item-name">{material.displayName}</span>
              <span className="item-meta">
                {kindLabel(material.kind)} · {humanSize(material.byteSize)}
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
