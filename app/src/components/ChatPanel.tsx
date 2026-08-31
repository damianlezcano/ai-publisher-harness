import { CreationCard } from "./CreationsPanel";
import { MaterialChip } from "./MaterialsPanel";
import type { AgentPhase, CreationView, MaterialView, MessageView } from "../types";
import { messages } from "../messages";

interface ChatPanelProps {
  projectId: string;
  messages: MessageView[];
  materials: MaterialView[];
  creations: CreationView[];
  agentPhase: AgentPhase;
  agentMessage: string | null;
  pendingUser?: { text: string; materialIds: string[] } | null;
  onRefresh?: () => void | Promise<void>;
}

function arraysEqual(a: string[], b: string[]): boolean {
  if (a.length !== b.length) return false;
  const sortedA = [...a].sort();
  const sortedB = [...b].sort();
  return sortedA.every((value, index) => value === sortedB[index]);
}

function MessageBubble({
  message,
  materialById,
  creationById,
  projectId,
  onRefresh,
}: {
  message: MessageView;
  materialById: Map<string, MaterialView>;
  creationById: Map<string, CreationView>;
  projectId: string;
  onRefresh?: () => void | Promise<void>;
}) {
  if (message.role === "user") {
    return (
      <div className="message message-user">
        <div className="message-header">
          <span className="message-role">{messages.timeline.userLabel}</span>
        </div>
        <p className="message-text">{message.text}</p>
        {message.materialIds.length > 0 && (
          <ul className="chip-list" aria-label={messages.assistant.attachmentsAriaLabel}>
            {message.materialIds.map((id) => {
              const material = materialById.get(id);
              return material ? (
                <li key={id}>
                  <MaterialChip projectId={projectId} material={material} />
                </li>
              ) : (
                <li key={id} className="chip">
                  {messages.assistant.attachmentFallback}
                </li>
              );
            })}
          </ul>
        )}
      </div>
    );
  }

  const isError = message.status === "failed" || message.status === "cancelled";

  return (
    <div className={`message message-assistant${isError ? " message-error" : ""}`}>
      <div className="message-header">
        <span className="message-role">{messages.timeline.assistantLabel}</span>
      </div>
      {isError ? (
        <p className="message-text" role="alert">
          {message.text}
        </p>
      ) : (
        <>
          <p className="message-text">{message.text}</p>
          {message.creationIds.length > 0 && (
            <div className="message-creations">
              {message.creationIds.map((id) => {
                const creation = creationById.get(id);
                if (!creation) return null;
                return (
                  <CreationCard
                    key={id}
                    projectId={projectId}
                    creation={creation}
                    onRefresh={onRefresh ?? (() => {})}
                  />
                );
              })}
            </div>
          )}
        </>
      )}
    </div>
  );
}

export default function ChatPanel({
  projectId,
  messages: messageList,
  materials,
  creations,
  agentPhase,
  agentMessage,
  pendingUser,
  onRefresh,
}: ChatPanelProps) {
  const materialById = new Map(materials.map((m) => [m.id, m]));
  const creationById = new Map(creations.map((c) => [c.id, c]));

  const hasPending =
    pendingUser != null &&
    !messageList.some(
      (m) =>
        m.role === "user" &&
        m.text === pendingUser.text &&
        arraysEqual(m.materialIds, pendingUser.materialIds),
    );

  return (
    <section className="panel chat" aria-label={messages.assistant.panelLabel}>
      <div className="chat-log" aria-live="polite">
        {messageList.length === 0 && !hasPending && agentPhase === "idle" && (
          <p className="muted">{messages.assistant.emptyHint}</p>
        )}
        {messageList.map((message) => (
          <MessageBubble
            key={message.id}
            message={message}
            materialById={materialById}
            creationById={creationById}
            projectId={projectId}
            onRefresh={onRefresh}
          />
        ))}
        {hasPending && pendingUser && (
          <div className="message message-user">
            <div className="message-header">
              <span className="message-role">{messages.timeline.userLabel}</span>
            </div>
            <p className="message-text">{pendingUser.text}</p>
            {pendingUser.materialIds.length > 0 && (
              <ul className="chip-list" aria-label={messages.assistant.attachmentsAriaLabel}>
                {pendingUser.materialIds.map((id) => {
                  const material = materialById.get(id);
                  return material ? (
                    <li key={id}>
                      <MaterialChip projectId={projectId} material={material} />
                    </li>
                  ) : (
                    <li key={id} className="chip">
                      {messages.assistant.attachmentFallback}
                    </li>
                  );
                })}
              </ul>
            )}
          </div>
        )}
        {agentPhase === "working" && (
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
    </section>
  );
}
