import { useEffect, useState } from "react";
import { api, errorMessage } from "../api";
import type { ModelSummary, ProjectView } from "../types";
import Dialog from "./ui/Dialog";
import { kindLabel, modelOptionLabel } from "../labels";
import { messages } from "../messages";

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
    try {
      if (value === "") {
        await api.conversationModelClear(project.id);
      } else {
        const [providerId, modelId] = value.split("::");
        await api.conversationModelSelect(project.id, providerId, modelId);
      }
      await onRefresh();
      setError(null);
    } catch (err) {
      setError(errorMessage(err));
    }
  }
  const current = project.model ? `${project.model.providerId}::${project.model.modelId}` : "";

  return (
    <Dialog title={messages.conversationDetails.title} onClose={onClose} closeButton>
      <section className="provider-section">
        <h3>{messages.conversationDetails.conversationHeading}</h3>
        <label htmlFor="conversation-name">{messages.conversationDetails.nameLabel}</label>
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
            {messages.conversationDetails.rename}
          </button>
        </div>
      </section>
      <section className="provider-section">
        <h3>{messages.conversationDetails.modelHeading}</h3>
        <label htmlFor="conversation-model">{messages.conversationDetails.modelLabel}</label>
        <select
          id="conversation-model"
          value={current}
          disabled={active}
          onChange={(event) => void changeModel(event.target.value)}
        >
          <option value="">{messages.conversationDetails.globalDefault}</option>
          {models.map((model) => (
            <option
              key={`${model.providerId}::${model.modelId}`}
              value={`${model.providerId}::${model.modelId}`}
            >
              {modelOptionLabel(model)}
            </option>
          ))}
        </select>
        {active && (
          <p className="notice">{messages.conversationDetails.activeTurnNotice}</p>
        )}
      </section>
      <section className="provider-section">
        <h3>{messages.conversationDetails.filesHeading}</h3>
        <h4>{messages.conversationDetails.uploadedHeading}</h4>
        {project.materials.length === 0 ? (
          <p className="muted">{messages.conversationDetails.noUploaded}</p>
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
                  {messages.conversationDetails.openContainingFolder}
                </button>
              </li>
            ))}
          </ul>
        )}
        <h4>{messages.conversationDetails.generatedHeading}</h4>
        {project.creations.length === 0 ? (
          <p className="muted">{messages.conversationDetails.noGenerated}</p>
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
                  {messages.conversationDetails.openContainingFolder}
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
