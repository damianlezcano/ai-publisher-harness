import { useState } from "react";
import { CreationCard, type CreationCardShareProps } from "./CreationsPanel";
import { MaterialChip } from "./MaterialsPanel";
import AssistantMetrics from "./AssistantMetrics";
import { turnMetricsByAssistantId } from "../turnMetricsBinding";
import type { AgentPhase, CreationView, MaterialView, MessageView, TurnMetrics } from "../types";
import { messages } from "../messages";

/**
 * A multi-file selection must not dominate the conversation with dozens of
 * expanded attachment cards. Above this threshold the attachment list is
 * collapsed into a compact "📎 N archivos adjuntos · Ver archivos" summary and
 * only expands on demand. Small selections keep the natural per-file display.
 */
const ATTACHMENT_COLLAPSE_THRESHOLD = 5;

interface ChatPanelProps {
  projectId: string;
  messages: MessageView[];
  materials: MaterialView[];
  creations: CreationView[];
  agentPhase: AgentPhase;
  agentMessage: string | null;
  onRefresh?: () => void | Promise<void>;
  share?: CreationCardShareProps;
  /** True when the accepted-import progress line already owns the in-flight
   * status (single merged "Procesando tu solicitud · …" line), so this panel
   * must not render its own duplicate "Procesando tu solicitud…" line. */
  suppressWorkingStatus?: boolean;
  /** True while a compact/generic per-item summary synthesis is running for a
   * no-attachment turn, so the working line shows a truthful synthesis label. */
  synthesizingSummaries?: boolean;
}

type TimelineItem = { kind: "message"; key: string; at: string; message: MessageView };

function buildTimeline(messageList: MessageView[]): TimelineItem[] {
  const items: TimelineItem[] = messageList.map((message) => ({
    kind: "message" as const,
    key: message.id,
    at: message.createdAt,
    message,
  }));
  items.sort((a, b) => {
    const byTime = a.at.localeCompare(b.at);
    return byTime !== 0 ? byTime : a.key.localeCompare(b.key);
  });
  return items;
}

function MessageAttachments({
  materialIds,
  materialById,
  projectId,
}: {
  materialIds: string[];
  materialById: Map<string, MaterialView>;
  projectId: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const collapsible = materialIds.length > ATTACHMENT_COLLAPSE_THRESHOLD;
  const collapsed = collapsible && !expanded;

  if (collapsed) {
    return (
      <button
        type="button"
        className="attachment-summary"
        aria-expanded={false}
        onClick={() => setExpanded(true)}
      >
        <span className="attachment-summary-label">
          {messages.timeline.attachmentsSummary(materialIds.length)}
        </span>
        <span className="attachment-summary-action">{messages.timeline.showAttachments}</span>
      </button>
    );
  }

  return (
    <>
      {collapsible && (
        <button
          type="button"
          className="attachment-summary-toggle"
          aria-expanded={true}
          onClick={() => setExpanded(false)}
        >
          {messages.timeline.hideAttachments}
        </button>
      )}
      <ul className="chip-list" aria-label={messages.assistant.attachmentsAriaLabel}>
        {materialIds.map((id) => {
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
    </>
  );
}

function MessageBubble({
  message,
  materialById,
  creationById,
  projectId,
  onRefresh,
  share,
  turnMetrics,
}: {
  message: MessageView;
  materialById: Map<string, MaterialView>;
  creationById: Map<string, CreationView>;
  projectId: string;
  onRefresh?: () => void | Promise<void>;
  share?: CreationCardShareProps;
  turnMetrics?: TurnMetrics | null;
}) {
  if (message.role === "user") {
    return (
      <div className="message message-user">
        <div className="message-header">
          <span className="message-role">{messages.timeline.userLabel}</span>
        </div>
        <p className="message-text">{message.text}</p>
        {message.materialIds.length > 0 && (
          <MessageAttachments
            materialIds={message.materialIds}
            materialById={materialById}
            projectId={projectId}
          />
        )}
      </div>
    );
  }

  const isError = message.status === "failed" || message.status === "cancelled";
  const text = message.text.trim();
  const hasCreations = message.creationIds.length > 0;
  if (!isError && text === "" && !hasCreations) {
    return null;
  }

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
          {text !== "" && <p className="message-text">{message.text}</p>}
          {hasCreations && (
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
          {turnMetrics && (
            <AssistantMetrics
              messageId={message.id}
              createdAt={message.createdAt}
              turnMetrics={turnMetrics}
            />
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
  onRefresh,
  share,
  suppressWorkingStatus = false,
  synthesizingSummaries = false,
}: ChatPanelProps) {
  const materialById = new Map(materials.map((m) => [m.id, m]));
  const creationById = new Map(creations.map((c) => [c.id, c]));
  const timeline = buildTimeline(messageList);
  const metricsByAssistant = turnMetricsByAssistantId(messageList);

  const isEmpty = messageList.length === 0 && agentPhase === "idle";

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
              turnMetrics={metricsByAssistant.get(item.message.id)}
            />
          ) : null,
        )}
        {agentPhase === "working" && !suppressWorkingStatus && (
          <p className="chat-status">
            <span className="spinner" aria-hidden="true" />
            {synthesizingSummaries
              ? messages.compactProgress.generatingSummariesIndeterminate
              : messages.agent.creating}
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
