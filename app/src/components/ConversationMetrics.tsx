import type { SessionKnowledgeMetrics, SessionLogEntry, SessionUsage } from "../types";
import { messages } from "../messages";

interface Props {
  conversationId: string;
  logs: SessionLogEntry[];
}

function latest<T>(
  logs: SessionLogEntry[],
  select: (entry: SessionLogEntry) => T | null | undefined,
): T | null {
  for (let index = logs.length - 1; index >= 0; index--) {
    const value = select(logs[index]);
    if (value) return value;
  }
  return null;
}

function unavailable(value: number | null | undefined, suffix = ""): string {
  return value == null
    ? messages.conversationDetails.metrics.unavailable
    : `${value.toLocaleString("es-AR")}${suffix}`;
}

function bytes(value: number | null | undefined): string {
  return unavailable(value, " bytes");
}

function duration(value: number | null | undefined): string {
  return unavailable(value, " ms");
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

function ProviderMetrics({ usage }: { usage: SessionUsage | null }) {
  const unavailableText = messages.conversationDetails.metrics.unavailable;
  // Provider token/cost fields are only truthful when the backend explicitly
  // identified them as provider telemetry. Local estimates never appear in
  // this "Uso real" subsection.
  const actual = usage?.source === "provider_actual" ? usage : null;
  return (
    <section
      className="conversation-metrics-subsection"
      aria-label={messages.conversationDetails.metrics.realProvider}
    >
      <h4>{messages.conversationDetails.metrics.realProvider}</h4>
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
          label={messages.conversationDetails.metrics.turnDuration}
          value={duration(usage?.turnDurationMs)}
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

function KnowledgeMetrics({ knowledge }: { knowledge: SessionKnowledgeMetrics | null }) {
  const unavailableText = messages.conversationDetails.metrics.unavailable;
  return (
    <section
      className="conversation-metrics-subsection"
      aria-label={messages.conversationDetails.metrics.knowledgeOptimization}
    >
      <h4>{messages.conversationDetails.metrics.knowledgeOptimization}</h4>
      <dl className="conversation-metrics-grid">
        <Metric
          label={messages.conversationDetails.metrics.materialCount}
          value={unavailable(knowledge?.materialCount)}
        />
        <Metric
          label={messages.conversationDetails.metrics.corpusBytes}
          value={bytes(knowledge?.corpusBytes)}
        />
        <Metric
          label={messages.conversationDetails.metrics.corpusChars}
          value={unavailable(knowledge?.corpusUtf8Chars, " caracteres")}
        />
        <Metric
          label={messages.conversationDetails.metrics.naiveCorpusTokens}
          value={unavailable(knowledge?.corpusEstTokens, " tokens estimados")}
        />
        <Metric
          label={messages.conversationDetails.metrics.candidateCount}
          value={unavailable(knowledge?.retrievalCandidateCount)}
        />
        <Metric
          label={messages.conversationDetails.metrics.selectedEvidenceCount}
          value={unavailable(knowledge?.selectedEvidenceCount)}
        />
        <Metric
          label={messages.conversationDetails.metrics.selectedEvidenceBytes}
          value={bytes(knowledge?.selectedEvidenceBytes)}
        />
        <Metric
          label={messages.conversationDetails.metrics.selectedEvidenceChars}
          value={unavailable(knowledge?.selectedEvidenceUtf8Chars, " caracteres")}
        />
        <Metric
          label={messages.conversationDetails.metrics.evidenceTokens}
          value={unavailable(knowledge?.evidenceEstTokens, " tokens estimados")}
        />
        <Metric
          label={messages.conversationDetails.metrics.contextReduction}
          value={unavailable(knowledge?.contextReductionPct, " %")}
        />
        <Metric
          label={messages.conversationDetails.metrics.semanticState}
          value={knowledge?.semanticProviderState || unavailableText}
        />
        <Metric
          label={messages.conversationDetails.metrics.preparationDuration}
          value={duration(knowledge?.requestPreparationMs)}
        />
      </dl>
      <p className="muted">{messages.conversationDetails.metrics.estimateNotice}</p>
    </section>
  );
}

export default function ConversationMetrics({ conversationId, logs }: Props) {
  const selected = logs.filter(
    (entry) =>
      entry.usage?.conversationId === conversationId ||
      entry.knowledge?.conversationId === conversationId,
  );
  const usage = latest(selected, (entry) => entry.usage);
  const knowledge = latest(selected, (entry) => entry.knowledge);

  return (
    <section
      className="provider-section conversation-metrics"
      aria-label={messages.conversationDetails.metrics.heading}
    >
      <h3>{messages.conversationDetails.metrics.heading}</h3>
      <h4>{messages.conversationDetails.metrics.lastTurn}</h4>
      <ProviderMetrics usage={usage} />
      <KnowledgeMetrics knowledge={knowledge} />
    </section>
  );
}
