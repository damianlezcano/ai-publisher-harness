import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type {
  AgentPhase,
  MaterialView,
  ModelSummary,
  ProviderSummary,
  SelectedModelView,
} from "../types";
import { messages } from "../messages";
import ErrorNotice from "./ui/ErrorNotice";

export interface ComposerBarProps {
  projectId: string;
  materials: MaterialView[];
  agentPhase: AgentPhase;
  aiUsable: boolean;
  onSend: (prompt: string, attachmentIds: string[]) => void | Promise<void>;
  onCancel: () => void | Promise<void>;
  onOpenProvider?: () => void;
  onMaterialsChanged?: () => void | Promise<void>;
  shareAction?: React.ReactNode;
  attachmentIds?: string[];
  onAttachmentIdsChange?: (ids: string[]) => void;
}

const TEXTAREA_MAX_HEIGHT_PX = 150;

function clipboardHasImage(items: DataTransferItemList): DataTransferItem | null {
  for (let i = 0; i < items.length; i++) {
    const item = items[i];
    if (item.kind === "file" && item.type.startsWith("image/")) {
      return item;
    }
  }
  return null;
}

function modelOptionLabel(model: ModelSummary): string {
  return `${model.name}${model.free ? messages.model.freeSuffix : messages.model.paidSuffix}`;
}

export default function ComposerBar({
  projectId,
  materials,
  agentPhase,
  aiUsable,
  onSend,
  onCancel,
  onMaterialsChanged,
  shareAction,
  attachmentIds: attachmentIdsProp,
  onAttachmentIdsChange,
}: ComposerBarProps) {
  const [prompt, setPrompt] = useState("");
  const [internalAttachmentIds, setInternalAttachmentIds] = useState<string[]>([]);
  const [pasteBusy, setPasteBusy] = useState(false);
  const [showMaterialPicker, setShowMaterialPicker] = useState(false);

  const controlled = attachmentIdsProp !== undefined;
  const attachmentIds = controlled ? attachmentIdsProp : internalAttachmentIds;

  function setAttachmentIds(next: string[] | ((prev: string[]) => string[])) {
    const resolved = typeof next === "function" ? next(attachmentIds) : next;
    if (controlled) {
      onAttachmentIdsChange?.(resolved);
    } else {
      setInternalAttachmentIds(resolved);
    }
  }
  const [models, setModels] = useState<ModelSummary[]>([]);
  const [providers, setProviders] = useState<ProviderSummary[]>([]);
  const [selected, setSelected] = useState<SelectedModelView | null>(null);
  const [modelLoading, setModelLoading] = useState(true);
  const [pickError, setPickError] = useState<unknown | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const working = agentPhase === "working";
  const composerDisabled = working || pasteBusy || !aiUsable;

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const [modelList, providerList, current] = await Promise.all([
          api.modelList(),
          api.providerList(),
          api.modelGetSelected(),
        ]);
        if (!active) return;
        setModels(modelList ?? []);
        setProviders(providerList ?? []);
        setSelected(current);
      } finally {
        if (active) setModelLoading(false);
      }
    })();
    return () => {
      active = false;
    };
  }, []);

  function resizeTextarea() {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    const nextHeight = Math.min(el.scrollHeight, TEXTAREA_MAX_HEIGHT_PX);
    el.style.height = `${nextHeight}px`;
    el.style.overflowY = el.scrollHeight > TEXTAREA_MAX_HEIGHT_PX ? "auto" : "hidden";
  }

  useEffect(() => {
    resizeTextarea();
  }, [prompt]);

  const materialById = new Map(materials.map((m) => [m.id, m]));

  function removeAttachment(materialId: string) {
    setAttachmentIds((prev) => prev.filter((id) => id !== materialId));
  }

  function toggleMaterial(materialId: string) {
    setAttachmentIds((prev) =>
      prev.includes(materialId) ? prev.filter((id) => id !== materialId) : [...prev, materialId],
    );
  }

  async function pickFile() {
    if (composerDisabled) return;
    setPickError(null);
    try {
      const path = await api.pickFile();
      if (!path) return;
      const material = await api.materialAddFromPath(projectId, path);
      await onMaterialsChanged?.();
      setAttachmentIds((prev) => (prev.includes(material.id) ? prev : [...prev, material.id]));
      setShowMaterialPicker(false);
    } catch (err) {
      setPickError(err);
    }
  }

  async function handleAttachClick() {
    if (composerDisabled) return;
    if (materials.length === 0) {
      await pickFile();
      return;
    }
    setShowMaterialPicker((open) => !open);
  }

  async function handlePaste(event: React.ClipboardEvent<HTMLTextAreaElement>) {
    const imageItem = clipboardHasImage(event.clipboardData.items);
    if (!imageItem) return;

    event.preventDefault();
    const file = imageItem.getAsFile();
    if (!file) return;

    setPasteBusy(true);
    try {
      const buffer = await file.arrayBuffer();
      const fileName = file.name || `captura-${Date.now()}.png`;
      const result = await api.materialAddImage(
        projectId,
        fileName,
        file.type || imageItem.type,
        new Uint8Array(buffer),
      );
      await onMaterialsChanged?.();
      setAttachmentIds((prev) =>
        prev.includes(result.material.id) ? prev : [...prev, result.material.id],
      );
    } finally {
      setPasteBusy(false);
    }
  }

  async function send() {
    const text = prompt.trim();
    if (text === "" || composerDisabled) return;
    const ids = attachmentIds;
    setPrompt("");
    setAttachmentIds([]);
    setShowMaterialPicker(false);
    await onSend(text, ids);
  }

  function handlePromptKeyDown(event: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void send();
    }
  }

  const connectedIds = new Set(providers.filter((p) => p.connected).map((p) => p.id));
  const visibleModels = models.filter((m) => m.free || connectedIds.has(m.providerId));

  const currentValue = selected?.model
    ? `${selected.model.providerId}::${selected.model.modelId}`
    : "";

  const valueInVisible = visibleModels.some(
    (m) => `${m.providerId}::${m.modelId}` === currentValue,
  );

  async function handleModelChange(next: string) {
    if (next === "") return;
    const [providerId, modelId] = next.split("::");
    try {
      await api.modelSelect(providerId, modelId);
      const current = await api.modelGetSelected();
      setSelected(current);
    } catch {
      // Compact bar keeps the previous selection; error surface is left to Settings.
    }
  }

  const selectedIsFree = selected?.model?.free === true && !selected.requiresChoice;

  return (
    <div className="composer-bar" role="region" aria-label={messages.assistant.panelLabel}>
      {pickError !== null && <ErrorNotice error={pickError} />}
      {attachmentIds.length > 0 && (
        <ul className="chip-list" aria-label={messages.assistant.attachmentsAriaLabel}>
          {attachmentIds.map((id) => {
            const material = materialById.get(id);
            const name = material?.displayName ?? messages.assistant.attachmentFallback;
            return (
              <li key={id} className="chip">
                <span>{name}</span>
                <button
                  type="button"
                  className="chip-remove"
                  aria-label={messages.assistant.removeAttachment(name)}
                  disabled={composerDisabled}
                  onClick={() => removeAttachment(id)}
                >
                  ×
                </button>
              </li>
            );
          })}
        </ul>
      )}

      <form
        className="composer-form"
        onSubmit={(e) => {
          e.preventDefault();
          void send();
        }}
      >
        <div className="composer-primary">
          {aiUsable && (
            <button
              type="button"
              className="ghost composer-attach"
              aria-label={messages.assistant.attachMaterial}
              aria-expanded={showMaterialPicker}
              disabled={composerDisabled}
              onClick={() => void handleAttachClick()}
            >
              <span aria-hidden="true">📎</span>
            </button>
          )}
          <label className="sr-only" htmlFor="composer-prompt">
            {messages.assistant.promptLabel}
          </label>
          <textarea
            ref={textareaRef}
            id="composer-prompt"
            className="composer-textarea"
            value={prompt}
            onChange={(e) => setPrompt(e.target.value)}
            onKeyDown={handlePromptKeyDown}
            onPaste={(e) => void handlePaste(e)}
            placeholder={messages.assistant.placeholder}
            rows={1}
            disabled={composerDisabled}
          />
          {working ? (
            <button type="button" className="danger" onClick={() => void onCancel()}>
              {messages.common.cancel}
            </button>
          ) : (
            <button
              type="submit"
              className="primary composer-send"
              disabled={prompt.trim() === "" || composerDisabled}
            >
              {messages.common.send}
            </button>
          )}
        </div>

        {showMaterialPicker && aiUsable && (
          <div className="composer-attach-menu">
            <button
              type="button"
              className="ghost composer-add-file"
              disabled={composerDisabled}
              onClick={() => void pickFile()}
            >
              {messages.material.addFile}
            </button>
            {materials.length > 0 && (
              <ul className="chip-list composer-material-picker">
                {materials.map((material) => {
                  const isSelected = attachmentIds.includes(material.id);
                  return (
                    <li key={material.id}>
                      <button
                        type="button"
                        className="chip"
                        aria-pressed={isSelected}
                        disabled={composerDisabled}
                        onClick={() => toggleMaterial(material.id)}
                      >
                        {material.displayName}
                      </button>
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        )}

        <div className="composer-secondary">
          <div className="composer-model">
            <label className="sr-only" htmlFor="composer-model-select">
              {messages.model.label}
            </label>
            {selected?.requiresChoice && (
              <p className="notice" role="alert">
                {selected.notice ?? messages.model.unavailableChoice}
              </p>
            )}
            <select
              id="composer-model-select"
              className="composer-model-select"
              value={valueInVisible ? currentValue : ""}
              onChange={(e) => void handleModelChange(e.target.value)}
              disabled={modelLoading || composerDisabled}
            >
              {modelLoading && <option value="">{messages.model.loading}</option>}
              {!modelLoading && visibleModels.length === 0 && (
                <option value="">{messages.model.none}</option>
              )}
              {!modelLoading && visibleModels.length > 0 && !valueInVisible && (
                <option value="" disabled>
                  {selectedIsFree ? messages.model.automaticFree : messages.model.choose}
                </option>
              )}
              {visibleModels.map((m) => (
                <option
                  key={`${m.providerId}::${m.modelId}`}
                  value={`${m.providerId}::${m.modelId}`}
                >
                  {modelOptionLabel(m)}
                </option>
              ))}
            </select>
          </div>

          {shareAction && <div className="composer-share-slot">{shareAction}</div>}
        </div>
      </form>
    </div>
  );
}
