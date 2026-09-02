import { useEffect, useState } from "react";
import { api, errorMessage } from "../api";
import type { ModelSummary, ProjectView } from "../types";
import Dialog from "./ui/Dialog";
import { kindLabel } from "../labels";

interface Props {
  project: ProjectView;
  active: boolean;
  onClose: () => void;
  onRefresh: () => void | Promise<void>;
}

export default function ConversationDetails({ project, active, onClose, onRefresh }: Props) {
  const [name, setName] = useState(project.name);
  const [models, setModels] = useState<ModelSummary[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void api
      .modelList()
      .then(setModels)
      .catch((err) => setError(errorMessage(err)));
  }, [project.id, project.name]);

  async function rename() {
    try {
      await api.projectRename(project.id, name);
      await onRefresh();
      setError(null);
    } catch (err) {
      setError(errorMessage(err));
    }
  }
  async function changeModel(value: string) {
    const [providerId, modelId] = value.split("::");
    try {
      await api.conversationModelSelect(project.id, providerId, modelId);
      await onRefresh();
      setError(null);
    } catch (err) {
      setError(errorMessage(err));
    }
  }
  const current = project.model ? `${project.model.providerId}::${project.model.modelId}` : "";

  return (
    <Dialog title="Detalles de la conversación" onClose={onClose} closeButton>
      <section className="provider-section">
        <h3>Conversación</h3>
        <label htmlFor="conversation-name">Nombre</label>
        <div className="row-actions wrap">
          <input
            id="conversation-name"
            value={name}
            onChange={(event) => setName(event.target.value)}
          />
          <button
            type="button"
            className="secondary"
            onClick={() => void rename()}
            disabled={active || name.trim() === project.name}
          >
            Renombrar
          </button>
        </div>
      </section>
      <section className="provider-section">
        <h3>Modelo</h3>
        <label htmlFor="conversation-model">Modelo de esta conversación</label>
        <select
          id="conversation-model"
          value={current}
          disabled={active}
          onChange={(event) => void changeModel(event.target.value)}
        >
          <option value="">Predeterminado de Configuración</option>
          {models.map((model) => (
            <option
              key={`${model.providerId}::${model.modelId}`}
              value={`${model.providerId}::${model.modelId}`}
            >
              {model.name}
              {model.free ? " · Gratis" : " · De pago"}
            </option>
          ))}
        </select>
        {active && (
          <p className="notice">Esperá a que termine la solicitud antes de cambiar el modelo.</p>
        )}
      </section>
      <section className="provider-section">
        <h3>Archivos y material</h3>
        <h4>Material subido</h4>
        {project.materials.length === 0 ? (
          <p className="muted">No hay material subido.</p>
        ) : (
          <ul className="item-list">
            {project.materials.map((material) => (
              <li key={material.id} className="item-row">
                <span>{material.displayName}</span>
                <button
                  type="button"
                  className="secondary"
                  onClick={() => void api.materialOpenFolder(project.id, material.id)}
                >
                  Abrir carpeta contenedora
                </button>
              </li>
            ))}
          </ul>
        )}
        <h4>Creaciones generadas</h4>
        {project.creations.length === 0 ? (
          <p className="muted">No hay creaciones generadas.</p>
        ) : (
          <ul className="item-list">
            {project.creations.map((creation) => (
              <li key={creation.id} className="item-row">
                <span>{kindLabel(creation.kind)}</span>
                <button
                  type="button"
                  className="secondary"
                  onClick={() => void api.creationOpenFolder(project.id, creation.id)}
                >
                  Abrir carpeta contenedora
                </button>
              </li>
            ))}
          </ul>
        )}
      </section>
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </Dialog>
  );
}
