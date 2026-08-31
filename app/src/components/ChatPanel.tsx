import { useState } from "react";
import { api, errorMessage } from "../api";
import type { AgentPhase, MaterialView } from "../types";

interface ChatPanelProps {
  projectId: string;
  materials: MaterialView[];
  agentPhase: AgentPhase;
  agentMessage: string | null;
  onRefresh: () => void | Promise<void>;
}

interface Turn {
  role: "user";
  text: string;
}

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
}: ChatPanelProps) {
  const [prompt, setPrompt] = useState("");
  const [turns, setTurns] = useState<Turn[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [attachmentIds, setAttachmentIds] = useState<string[]>([]);
  const [pasteBusy, setPasteBusy] = useState(false);

  const working = agentPhase === "working";

  const materialById = new Map(materials.map((m) => [m.id, m]));

  function removeAttachment(materialId: string) {
    setAttachmentIds((prev) => prev.filter((id) => id !== materialId));
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
      setError(errorMessage(err));
    } finally {
      setPasteBusy(false);
    }
  }

  async function send() {
    const text = prompt.trim();
    if (text === "") return;
    setError(null);
    const ids = attachmentIds;
    setPrompt("");
    setAttachmentIds([]);
    setTurns((prev) => [...prev, { role: "user", text }]);
    try {
      await api.agentSend(projectId, text, ids);
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  async function cancel() {
    setError(null);
    try {
      await api.agentCancel(projectId);
      onRefresh();
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  return (
    <section className="panel chat" aria-label="Conversación">
      <h2>Conversación</h2>

      <div className="chat-log" aria-live="polite">
        {turns.length === 0 && agentPhase === "idle" && (
          <p className="muted">Describí qué recurso querés crear.</p>
        )}
        {turns.map((turn, index) => (
          <p key={index} className="chat-user">
            {turn.text}
          </p>
        ))}
        {working && <p className="chat-status">Creando tu recurso…</p>}
        {agentPhase === "completed" && agentMessage && (
          <p className="chat-status ok">{agentMessage}</p>
        )}
        {agentPhase === "failed" && agentMessage && (
          <p className="chat-status err" role="alert">
            {agentMessage}
          </p>
        )}
      </div>

      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}

      {attachmentIds.length > 0 && (
        <ul className="chip-list" aria-label="Archivos adjuntos">
          {attachmentIds.map((id) => {
            const material = materialById.get(id);
            const name = material?.displayName ?? "Archivo adjunto";
            return (
              <li key={id} className="chip">
                <span>{name}</span>
                <button
                  type="button"
                  className="chip-remove"
                  aria-label={`Quitar ${name}`}
                  disabled={working || pasteBusy}
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
          Pedido a la IA
        </label>
        <textarea
          id="prompt-input"
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          onPaste={(e) => void handlePaste(e)}
          placeholder="Ej.: Creá una actividad interactiva sobre la fotosíntesis"
          rows={3}
          disabled={working || pasteBusy}
        />
        <div className="chat-actions">
          {working ? (
            <button type="button" className="danger" onClick={() => void cancel()}>
              Cancelar
            </button>
          ) : (
            <button type="submit" className="primary" disabled={prompt.trim() === "" || pasteBusy}>
              Enviar
            </button>
          )}
        </div>
      </form>
    </section>
  );
}
