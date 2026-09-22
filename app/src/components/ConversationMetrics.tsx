import type {
  ConversationUsageTotals,
  MessageView,
  SessionLogEntry,
  SessionUsage,
  TurnMetrics,
} from "../types";
import { messages } from "../messages";

interface Props {
  conversationId: string;
  logs: SessionLogEntry[];
  durableMetrics: TurnMetrics | null;
  accumulatedUsage: ConversationUsageTotals | null;
  /** Persisted conversation messages for durable Knowledge count derivation. */
  conversationMessages: MessageView[];
}

function unavailable(value: number | null | undefined, suffix = ""): string {
  return value == null
    ? messages.conversationDetails.metrics.unavailable
    : `${value.toLocaleString("es-AR")}${suffix}`;
}

function cost(value: number | null | undefined): string {
  return value == null
    ? messages.conversationDetails.metrics.unavailable
    : `USD ${value.toLocaleString("en-US", { maximumFractionDigits: 6 })}`;
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div className="conversation-metric">
      <dt>{label}</dt>
      <dd title={value}>{value}</dd>
    </div>
  );
}

function ProviderMetrics({
  usage,
  heading = messages.conversationDetails.metrics.realProvider,
}: {
  usage: SessionUsage | null;
  heading?: string;
}) {
  const unavailableText = messages.conversationDetails.metrics.unavailable;
  const actual = usage?.source === "provider_actual" ? usage : null;
  return (
    <section
      className="conversation-metrics-subsection"
      aria-label={messages.conversationDetails.metrics.realProvider}
    >
      <h4>{heading}</h4>
      <dl className="conversation-metrics-grid">
        <Metric
          label={messages.conversationDetails.metrics.provider}
          value={usage?.provider || unavailableText}
        />
        <Metric
          label={messages.conversationDetails.metrics.model}
          value={usage?.model || unavailableText}
        />
        <Metric
          label={messages.conversationDetails.metrics.inputTokens}
          value={unavailable(actual?.inputTokens, " tokens")}
        />
        <Metric
          label={messages.conversationDetails.metrics.outputTokens}
          value={unavailable(actual?.outputTokens, " tokens")}
        />
        <Metric
          label={messages.conversationDetails.metrics.cacheReadTokens}
          value={unavailable(actual?.cacheReadTokens, " tokens")}
        />
        <Metric
          label={messages.conversationDetails.metrics.cacheWriteTokens}
          value={unavailable(actual?.cacheWriteTokens, " tokens")}
        />
        <Metric
          label={messages.conversationDetails.metrics.actualCost}
          value={cost(actual?.costUsd)}
        />
        <Metric
          label={messages.conversationDetails.metrics.remoteCalls}
          value={unavailable(usage?.remoteCalls ?? null)}
        />
      </dl>
      {usage?.additionalAttachmentRoute && (
        <p className="metrics-warning" role="status">
          {messages.conversationDetails.metrics.additionalAttachmentRoute}
        </p>
      )}
    </section>
  );
}

function KnowledgeSummary({
  knowledgeResponseCount,
  materialCount,
}: {
  knowledgeResponseCount: number;
  materialCount: number | null;
}) {
  if (knowledgeResponseCount === 0) {
    return (
      <section
        className="conversation-metrics-subsection"
        aria-label={messages.conversationDetails.metrics.knowledgeOptimization}
      >
        <h4>{messages.conversationDetails.metrics.knowledgeOptimization}</h4>
        <p className="muted">
          {messages.conversationDetails.metrics.knowledgeNotUsedInConversation}
        </p>
      </section>
    );
  }
  return (
    <section
      className="conversation-metrics-subsection"
      aria-label={messages.conversationDetails.metrics.knowledgeOptimization}
    >
      <h4>{messages.conversationDetails.metrics.knowledgeOptimization}</h4>
      <dl className="conversation-metrics-grid">
        <Metric
          label={messages.conversationDetails.metrics.knowledgeLabel}
          value={messages.conversationDetails.metrics.knowledgeUsedInResponses(
            knowledgeResponseCount,
          )}
        />
        <Metric
          label={messages.conversationDetails.metrics.materialsUsedLabel}
          value={
            materialCount != null
              ? messages.conversationDetails.metrics.materialsUsedValue(materialCount)
              : messages.conversationDetails.metrics.unavailable
          }
        />
      </dl>
    </section>
  );
}

export default function ConversationMetrics({
  conversationId,
  logs,
  durableMetrics,
  accumulatedUsage,
  conversationMessages,
}: Props) {
  const accumulated = accumulatedUsage
    ? {
        conversationId,
        turnId: "",
        provider: accumulatedUsage.provider || "",
        model: accumulatedUsage.model || "",
        inputTokens: accumulatedUsage.inputTokens,
        outputTokens: accumulatedUsage.outputTokens,
        cacheReadTokens: accumulatedUsage.cacheReadTokens,
        cacheWriteTokens: accumulatedUsage.cacheWriteTokens,
        totalTokens: accumulatedUsage.totalTokens,
        costUsd: accumulatedUsage.costUsd,
        turnDurationMs: accumulatedUsage.turnDurationMs,
        source: accumulatedUsage.source || "unavailable",
        remoteCalls: accumulatedUsage.remoteCalls,
      }
    : null;

  // Durable Knowledge count: derive from persisted per-turn metrics on
  // conversation messages (survives reload). Fall back to session logs.
  const knowledgeResponseCount =
    countKnowledgeFromMessages(conversationMessages) ||
    countKnowledgeFromLogs(logs, conversationId);
  const materialCount = durableMetrics?.materialCount ?? null;

  return (
    <section
      className="provider-section conversation-metrics"
      aria-label={messages.conversationDetails.metrics.heading}
    >
      <h3>{messages.conversationDetails.metrics.heading}</h3>
      <h4>{messages.conversationDetails.metrics.accumulated}</h4>
      <ProviderMetrics
        usage={accumulated}
        heading={messages.conversationDetails.metrics.accumulatedProvider}
      />
      <KnowledgeSummary
        knowledgeResponseCount={knowledgeResponseCount}
        materialCount={materialCount}
      />
    </section>
  );
}

function isKnowledgeTurn(m: TurnMetrics): boolean {
  return m.retrievalMode != null || m.localMode != null;
}

function countKnowledgeFromMessages(messageList: MessageView[]): number {
  // Build user-turn metrics map (same shape as turnMetricsByAssistantId).
  const metricsByUserId = new Map<string, TurnMetrics>();
  for (const msg of messageList) {
    if (msg.role === "user" && msg.turnMetrics) {
      metricsByUserId.set(msg.id, msg.turnMetrics);
    }
  }
  // Only count turns with a successful assistant response linked via turnId.
  let count = 0;
  for (const msg of messageList) {
    if (msg.role !== "assistant") continue;
    if (msg.status !== "ok") continue;
    if (!msg.turnId) continue;
    const userMetrics = metricsByUserId.get(msg.turnId);
    if (userMetrics && isKnowledgeTurn(userMetrics)) count++;
  }
  return count;
}

function countKnowledgeFromLogs(logs: SessionLogEntry[], conversationId: string): number {
  const selected = logs.filter((entry) => entry.knowledge?.conversationId === conversationId);
  return selected.filter((entry) => {
    const k = entry.knowledge;
    return k != null && (k.retrievalMode != null || k.localMode != null);
  }).length;
}
