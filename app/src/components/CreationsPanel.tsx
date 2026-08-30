import { useState } from "react";
import { api, errorMessage } from "../api";
import { humanSize, kindLabel, visibilityLabel } from "../labels";
import type { CreationView } from "../types";

interface CreationsPanelProps {
  projectId: string;
  creations: CreationView[];
  onRefresh: () => void;
}

export default function CreationsPanel({ projectId, creations, onRefresh }: CreationsPanelProps) {
  const [error, setError] = useState<string | null>(null);
  const [busyId, setBusyId] = useState<string | null>(null);

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

  return (
    <section className="panel" aria-label="Creaciones">
      <h2>Creaciones</h2>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      {creations.length === 0 ? (
        <p className="muted">Todavía no hay creaciones. Pedile algo a la IA.</p>
      ) : (
        <ul className="item-list">
          {creations.map((creation) => (
            <li key={creation.id} className="item-row">
              <span className="item-name">{creation.displayName}</span>
              <span className="item-meta">
                {kindLabel(creation.kind)} · {visibilityLabel(creation.visibility)} ·{" "}
                {humanSize(creation.byteSize)}
              </span>
              <span className="row-actions">
                <button
                  type="button"
                  className="secondary"
                  disabled={busyId === creation.id}
                  onClick={() => void toggle(creation)}
                >
                  {creation.visibility === "public" ? "Marcar privado" : "Se compartirá"}
                </button>
                <button type="button" className="primary" onClick={() => void open(creation.id)}>
                  Abrir
                </button>
              </span>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
