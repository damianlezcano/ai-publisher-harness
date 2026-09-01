import { CreationCard, type CreationCardShareProps } from "./CreationsPanel";
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
  share?: CreationCardShareProps;
}

function arraysEqual(a: string[], b: string[]): boolean {
  if (a.length !== b.length) return false;
  const sortedA = [...a].sort();
  const sortedB = [...b].sort();
  return sortedA.every((value, index) => value === sortedB[index]);
}

function referencedMaterialIds(
  messageList: MessageView[],
  pendingUser?: { materialIds: string[] } | null,
): Set<string> {
  const ids = new Set<string>();
  for (const message of messageList) {
    for (const id of message.materialIds) {
      ids.add(id);
    }
  }
  if (pendingUser) {
    for (const id of pendingUser.materialIds) {
      ids.add(id);
    }
  }
  return ids;
}

type TimelineItem =
  | { kind: "message"; key: string; at: string; message: MessageView }
  | { kind: "material"; key: string; at: string; material: MaterialView };

function buildTimeline(
  messageList: MessageView[],
  materials: MaterialView[],
  pendingUser?: { materialIds: string[] } | null,
): TimelineItem[] {
  const attached = referencedMaterialIds(messageList, pendingUser);
  const items: TimelineItem[] = [
    ...messageList.map((message) => ({
      kind: "message" as const,
      key: message.id,
      at: message.createdAt,
      message,
    })),
    ...materials
      .filter((material) => !attached.has(material.id))
      .map((material) => ({
        kind: "material" as const,
        key: material.id,
        at: material.createdAt,
        material,
      })),
  ];
  items.sort((a, b) => {
    const byTime = a.at.localeCompare(b.at);
    return byTime !== 0 ? byTime : a.key.localeCompare(b.key);
  });
  return items;
}

function MessageBubble({
  message,
  materialById,
  creationById,
  projectId,
  onRefresh,
  share,
}: {
  message: MessageView;
  materialById: Map<string, MaterialView>;
  creationById: Map<string, CreationView>;
  projectId: string;
  onRefresh?: () => void | Promise<void>;
  share?: CreationCardShareProps;
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
                    share={share}
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
  share,
}: ChatPanelProps) {
  const materialById = new Map(materials.map((m) => [m.id, m]));
  const creationById = new Map(creations.map((c) => [c.id, c]));
  const timeline = buildTimeline(messageList, materials, pendingUser);
  const unattachedCount = timeline.filter((item) => item.kind === "material").length;

  const hasPending =
    pendingUser != null &&
    !messageList.some(
      (m) =>
        m.role === "user" &&
        m.text === pendingUser.text &&
        arraysEqual(m.materialIds, pendingUser.materialIds),
    );

  const isEmpty =
    messageList.length === 0 && !hasPending && agentPhase === "idle" && unattachedCount === 0;

  const lastMessage = timeline.filter((i) => i.kind === "message").slice(-1)[0];
  const hasPersistedFailure =
    agentMessage != null &&
    lastMessage?.kind === "message" &&
    lastMessage.message.role === "assistant" &&
    (lastMessage.message.status === "failed" || lastMessage.message.status === "cancelled") &&
    lastMessage.message.text === agentMessage;

  return (
    <section className="panel chat" aria-label={messages.assistant.panelLabel}>
      <div className="chat-log" aria-live="polite">
        {isEmpty && <p className="muted chat-empty">{messages.assistant.emptyHint}</p>}
        {timeline.map((item) =>
          item.kind === "message" ? (
            <MessageBubble
              key={item.key}
              message={item.message}
              materialById={materialById}
              creationById={creationById}
              projectId={projectId}
              onRefresh={onRefresh}
              share={share}
            />
          ) : (
            <div key={item.key} className="message message-user message-attachment-only">
              <MaterialChip projectId={projectId} material={item.material} />
            </div>
          ),
        )}
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
        {agentPhase === "failed" && agentMessage && !hasPersistedFailure && (
          <p className="chat-status err" role="alert">
            {agentMessage}
          </p>
        )}
      </div>
    </section>
  );
}
