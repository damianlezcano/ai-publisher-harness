import { useState } from "react";
import { api, errorMessage } from "../api";
import type { AgentPhase } from "../types";

interface ChatPanelProps {
  projectId: string;
  agentPhase: AgentPhase;
  agentMessage: string | null;
  onRefresh: () => void;
}

interface Turn {
  role: "user";
  text: string;
}

export default function ChatPanel({
  projectId,
  agentPhase,
  agentMessage,
  onRefresh,
}: ChatPanelProps) {
  const [prompt, setPrompt] = useState("");
  const [turns, setTurns] = useState<Turn[]>([]);
  const [error, setError] = useState<string | null>(null);

  const working = agentPhase === "working";

  async function send() {
    const text = prompt.trim();
    if (text === "") return;
    setError(null);
    setPrompt("");
    setTurns((prev) => [...prev, { role: "user", text }]);
    try {
      await api.agentSend(projectId, text);
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
          placeholder="Ej.: Creá una actividad interactiva sobre la fotosíntesis"
          rows={3}
          disabled={working}
        />
        <div className="chat-actions">
          {working ? (
            <button type="button" className="danger" onClick={() => void cancel()}>
              Cancelar
            </button>
          ) : (
            <button type="submit" className="primary" disabled={prompt.trim() === ""}>
              Enviar
            </button>
          )}
        </div>
      </form>
    </section>
  );
}
