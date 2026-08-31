import { useEffect, useRef, useState } from "react";
import { api } from "../api";
import type { GuidanceActionKind } from "../guidance";
import type { AgentPhase, MaterialView } from "../types";
import { messages } from "../messages";
import EmptyState from "./ui/EmptyState";
import ErrorNotice from "./ui/ErrorNotice";

interface ChatPanelProps {
  projectId: string;
  materials: MaterialView[];
  agentPhase: AgentPhase;
  agentMessage: string | null;
  onRefresh: () => void | Promise<void>;
  aiUsable?: boolean;
  onOpenProvider?: () => void;
}

interface Turn {
  role: "user";
  text: string;
}

interface SendAttempt {
  text: string;
  attachmentIds: string[];
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

export default function ChatPanel({
  projectId,
  materials,
  agentPhase,
  agentMessage,
  onRefresh,
  aiUsable = true,
  onOpenProvider,
}: ChatPanelProps) {
  const [prompt, setPrompt] = useState("");
  const [turns, setTurns] = useState<Turn[]>([]);
  const [error, setError] = useState<unknown>(null);
  const [attachmentIds, setAttachmentIds] = useState<string[]>([]);
  const [pasteBusy, setPasteBusy] = useState(false);
  const [showMaterialPicker, setShowMaterialPicker] = useState(false);
  const [lastAttempt, setLastAttempt] = useState<SendAttempt | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const working = agentPhase === "working";
  const composerDisabled = working || pasteBusy || !aiUsable;

  const materialById = new Map(materials.map((m) => [m.id, m]));

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

  function removeAttachment(materialId: string) {
    setAttachmentIds((prev) => prev.filter((id) => id !== materialId));
  }

  function toggleMaterial(materialId: string) {
    setAttachmentIds((prev) =>
      prev.includes(materialId) ? prev.filter((id) => id !== materialId) : [...prev, materialId],
    );
  }

  async function handlePaste(event: React.ClipboardEvent<HTMLTextAreaElement>) {
    const imageItem = clipboardHasImage(event.clipboardData.items);
    if (!imageItem) return;

    event.preventDefault();
    const file = imageItem.getAsFile();
    if (!file) return;

    setPasteBusy(true);
    setError(null);
    try {
      const buffer = await file.arrayBuffer();
      const fileName = file.name || `captura-${Date.now()}.png`;
      const result = await api.materialAddImage(
        projectId,
        fileName,
        file.type || imageItem.type,
        new Uint8Array(buffer),
      );
      await onRefresh();
      setAttachmentIds((prev) =>
        prev.includes(result.material.id) ? prev : [...prev, result.material.id],
      );
    } catch (err) {
      setError(err);
    } finally {
      setPasteBusy(false);
    }
  }

  async function send() {
    const text = prompt.trim();
    if (text === "" || composerDisabled) return;
    setError(null);
    const ids = attachmentIds;
    setLastAttempt({ text, attachmentIds: ids });
    setPrompt("");
    setAttachmentIds([]);
    setShowMaterialPicker(false);
    setTurns((prev) => [...prev, { role: "user", text }]);
    try {
      await api.agentSend(projectId, text, ids);
    } catch (err) {
      setError(err);
    }
  }

  async function retrySend() {
    if (!lastAttempt) return;
    setError(null);
    try {
      await api.agentSend(projectId, lastAttempt.text, lastAttempt.attachmentIds);
    } catch (err) {
      setError(err);
    }
  }

  function handleErrorAction(kind: GuidanceActionKind) {
    if (kind === "retry") {
      void retrySend();
    } else if (kind === "connect-ai") {
      onOpenProvider?.();
    }
  }

  async function cancel() {
    setError(null);
    try {
      await api.agentCancel(projectId);
      onRefresh();
    } catch (err) {
      setError(err);
    }
  }

  function handlePromptKeyDown(event: React.KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey)) {
      event.preventDefault();
      void send();
    }
  }

  return (
    <section className="panel chat" aria-label={messages.assistant.panelLabel}>
      <h2>{messages.assistant.heading}</h2>

      <div className="chat-log" aria-live="polite">
        {turns.length === 0 && agentPhase === "idle" && (
          <p className="muted">{messages.assistant.emptyHint}</p>
        )}
        {turns.map((turn, index) => (
          <p key={index} className="chat-user">
            {turn.text}
          </p>
        ))}
        {working && (
          <p className="chat-status">
            <span className="spinner" aria-hidden="true" />
            {messages.agent.creating}
          </p>
        )}
        {agentPhase === "completed" && agentMessage && (
          <p className="chat-status ok">{agentMessage}</p>
        )}
        {agentPhase === "failed" && agentMessage && (
          <p className="chat-status err" role="alert">
            {agentMessage}
          </p>
        )}
      </div>

      {error !== null && <ErrorNotice error={error} onAction={handleErrorAction} />}

      {!aiUsable && (
        <EmptyState
          title={messages.assistant.noAi.title}
          actionLabel={messages.assistant.noAi.action}
          onAction={onOpenProvider}
        />
      )}

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
        className="chat-form"
        onSubmit={(e) => {
          e.preventDefault();
          void send();
        }}
      >
        <label className="sr-only" htmlFor="prompt-input">
          {messages.assistant.promptLabel}
        </label>
        <textarea
          ref={textareaRef}
          id="prompt-input"
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          onKeyDown={handlePromptKeyDown}
          onPaste={(e) => void handlePaste(e)}
          placeholder={messages.assistant.placeholder}
          rows={1}
          disabled={composerDisabled}
        />
        {showMaterialPicker && aiUsable && (
          <ul className="chip-list">
            {materials.map((material) => {
              const selected = attachmentIds.includes(material.id);
              return (
                <li key={material.id}>
                  <button
                    type="button"
                    className="chip"
                    aria-pressed={selected}
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
        <div className="chat-actions">
          {working ? (
            <button type="button" className="danger" onClick={() => void cancel()}>
              {messages.common.cancel}
            </button>
          ) : (
            <>
              {aiUsable && (
                <button
                  type="button"
                  className="secondary"
                  aria-expanded={showMaterialPicker}
                  disabled={composerDisabled}
                  onClick={() => setShowMaterialPicker((open) => !open)}
                >
                  {messages.assistant.attachMaterial}
                </button>
              )}
              <button
                type="submit"
                className="primary"
                disabled={prompt.trim() === "" || composerDisabled}
              >
                {messages.common.send}
              </button>
            </>
          )}
        </div>
      </form>
    </section>
  );
}
