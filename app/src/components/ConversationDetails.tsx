import { useEffect, useState } from "react";
import { api, errorMessage } from "../api";
import type { ModelSummary, PreviewData, ProjectView, ProviderSummary } from "../types";
import Dialog from "./ui/Dialog";
import PreviewModal from "./PreviewModal";
import { humanDate, humanSize, kindLabel, modelOptionLabel } from "../labels";
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
  const [providers, setProviders] = useState<ProviderSummary[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [preview, setPreview] = useState<{
    title: string;
    data: PreviewData;
    meta: { name: string; byteSize: number; kind: string };
    openExternal: () => void;
  } | null>(null);

  useEffect(() => {
    void Promise.all([api.modelList(), api.providerList()])
      .then(([modelList, providerList]) => {
        setModels(modelList);
        setProviders(providerList);
      })
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
  const connected = new Set(providers.filter((provider) => provider.connected).map((p) => p.id));
  const visibleModels = models.filter((model) => model.free || connected.has(model.providerId));

  async function openMaterial(material: {
    id: string;
    displayName: string;
    kind: string;
    byteSize: number;
  }) {
    setError(null);
    try {
      const data = await api.previewData(project.id, "material", material.id);
      setPreview({
        title: material.displayName,
        data,
        meta: { name: material.displayName, byteSize: material.byteSize, kind: material.kind },
        openExternal: () => {
          setPreview(null);
          void api.materialOpen(project.id, material.id);
        },
      });
    } catch {
      void api.materialOpen(project.id, material.id);
    }
  }

  async function openCreation(creation: {
    id: string;
    displayName: string;
    kind: string;
    byteSize: number;
  }) {
    setError(null);
    try {
      if (creation.kind === "web") {
        await api.previewOpenWeb(project.id, creation.id);
        return;
      }
      const data = await api.previewData(project.id, "creation", creation.id);
      setPreview({
        title: creation.displayName,
        data,
        meta: { name: creation.displayName, byteSize: creation.byteSize, kind: creation.kind },
        openExternal: () => {
          setPreview(null);
          void api.creationOpen(project.id, creation.id);
        },
      });
    } catch {
      void api.creationOpen(project.id, creation.id);
    }
  }

  return (
    <Dialog
      title={messages.conversationDetails.title}
      onClose={onClose}
      className="conversation-details-dialog"
      closeButton
    >
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
          {visibleModels.map((model) => (
            <option
              key={`${model.providerId}::${model.modelId}`}
              value={`${model.providerId}::${model.modelId}`}
            >
              {modelOptionLabel(model)}
            </option>
          ))}
        </select>
        {active && <p className="notice">{messages.conversationDetails.activeTurnNotice}</p>}
      </section>
      <section className="provider-section">
        <h3>{messages.conversationDetails.filesHeading}</h3>
        <div className="section-heading">
          <h4>{messages.conversationDetails.uploadedHeading}</h4>
          {project.materials.length > 0 && (
            <button
              type="button"
              className="secondary"
              onClick={() => void api.materialsOpenFolder(project.id)}
            >
              {messages.conversationDetails.openContainingFolder}
            </button>
          )}
        </div>
        {project.materials.length === 0 ? (
          <p className="muted">{messages.conversationDetails.noUploaded}</p>
        ) : (
          <ul className="item-list">
            {project.materials.map((material) => (
              <li key={material.id} className="item-row">
                <div className="item-row-main">
                  <span className="item-name">{material.displayName}</span>
                  <button
                    type="button"
                    className="secondary item-open"
                    aria-label={`${messages.common.open}: ${material.displayName}`}
                    onClick={() => void openMaterial(material)}
                  >
                    {messages.common.open}
                  </button>
                </div>
                <span className="item-meta">
                  {humanSize(material.byteSize)} · {humanDate(material.createdAt)}
                </span>
              </li>
            ))}
          </ul>
        )}
        <div className="section-heading">
          <h4>{messages.conversationDetails.generatedHeading}</h4>
          {project.creations.length > 0 && (
            <button
              type="button"
              className="secondary"
              onClick={() => void api.creationsOpenFolder(project.id)}
            >
              {messages.conversationDetails.openContainingFolder}
            </button>
          )}
        </div>
        {project.creations.length === 0 ? (
          <p className="muted">{messages.conversationDetails.noGenerated}</p>
        ) : (
          <ul className="item-list">
            {project.creations.map((creation) => (
              <li key={creation.id} className="item-row">
                <div className="item-row-main">
                  <span className="item-name">{creation.displayName}</span>
                  <button
                    type="button"
                    className="secondary item-open"
                    aria-label={`${messages.common.open}: ${creation.displayName}`}
                    onClick={() => void openCreation(creation)}
                  >
                    {messages.common.open}
                  </button>
                </div>
                <span className="item-meta">
                  {kindLabel(creation.kind)} · {humanSize(creation.byteSize)} ·{" "}
                  {humanDate(creation.createdAt)}
                </span>
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
      {preview && (
        <PreviewModal
          title={preview.title}
          preview={preview.data}
          meta={preview.meta}
          onClose={() => setPreview(null)}
          onOpenExternal={preview.openExternal}
        />
      )}
    </Dialog>
  );
}
