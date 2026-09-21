import type {
  ConversationUsageTotals,
  SessionKnowledgeMetrics,
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

function ProviderMetrics({
  usage,
  heading = messages.conversationDetails.metrics.realProvider,
}: {
  usage: SessionUsage | null;
  heading?: string;
}) {
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
        <Metric
          label={messages.conversationDetails.metrics.retrievalMode}
          value={knowledge?.retrievalMode || unavailableText}
        />
        {knowledge?.localMode && (
          <Metric
            label={messages.conversationDetails.metrics.localMode}
            value={knowledge.localMode}
          />
        )}
        <Metric
          label={messages.conversationDetails.metrics.exhaustiveCoverage}
          value={knowledge?.exhaustiveCoverage || unavailableText}
        />
        {knowledge?.retrievalMode === "exhaustive" && (
          <>
            <Metric
              label={messages.conversationDetails.metrics.eligibleMaterials}
              value={unavailable(knowledge.eligibleMaterials ?? null)}
            />
            <Metric
              label={messages.conversationDetails.metrics.materialsInspected}
              value={unavailable(knowledge.materialsInspected ?? null)}
            />
            <Metric
              label={messages.conversationDetails.metrics.chunksInspected}
              value={unavailable(knowledge.chunksInspected ?? null)}
            />
            <Metric
              label={messages.conversationDetails.metrics.lexicalHits}
              value={unavailable(knowledge.lexicalHits ?? null)}
            />
            <Metric
              label={messages.conversationDetails.metrics.semanticHits}
              value={unavailable(knowledge.semanticHits ?? null)}
            />
          </>
        )}
        {knowledge?.retrievalMode === "thematic" && (
          <>
            <Metric
              label={messages.conversationDetails.metrics.eligibleMaterials}
              value={unavailable(knowledge.eligibleMaterials ?? null)}
            />
            <Metric
              label={messages.conversationDetails.metrics.thematicContributingSources}
              value={unavailable(knowledge.materialsInspected ?? null)}
            />
            <Metric
              label={messages.conversationDetails.metrics.thematicCandidates}
              value={unavailable(knowledge.retrievalCandidateCount ?? null)}
            />
            <Metric
              label={messages.conversationDetails.metrics.selectedEvidenceCount}
              value={unavailable(knowledge.selectedEvidenceCount ?? null)}
            />
            <Metric
              label={messages.conversationDetails.metrics.evidenceTokens}
              value={unavailable(knowledge.evidenceEstTokens ?? null)}
            />
          </>
        )}
      </dl>
      <p className="muted">{messages.conversationDetails.metrics.estimateNotice}</p>
    </section>
  );
}

export default function ConversationMetrics({
  conversationId,
  logs,
  durableMetrics,
  accumulatedUsage,
}: Props) {
  // Durable metrics (persisted in project.json) are the authoritative source.
  // Fall back to session_logs when durable metrics are unavailable
  // (old projects, in-flight sessions before restart).
  const usage = durableUsage(durableMetrics, logs, conversationId);
  const knowledge = durableKnowledge(durableMetrics, logs, conversationId);
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

  return (
    <section
      className="provider-section conversation-metrics"
      aria-label={messages.conversationDetails.metrics.heading}
    >
      <h3>{messages.conversationDetails.metrics.heading}</h3>
      <h4>{messages.conversationDetails.metrics.lastTurn}</h4>
      <ProviderMetrics usage={usage} />
      <KnowledgeMetrics knowledge={knowledge} />
      <h4>{messages.conversationDetails.metrics.accumulated}</h4>
      <ProviderMetrics
        usage={accumulated}
        heading={messages.conversationDetails.metrics.accumulatedProvider}
      />
    </section>
  );
}

function durableUsage(
  durable: TurnMetrics | null,
  logs: SessionLogEntry[],
  conversationId: string,
): SessionUsage | null {
  // `source`/`remoteCalls` can be the only available provider facts (for
  // example, a completed zero-source K6 turn). It is still the authoritative
  // durable record and must not be replaced with a stale session-log entry.
  if (durable && Object.values(durable).some((value) => value != null)) {
    return {
      conversationId,
      turnId: "",
      provider: durable.provider || "",
      model: durable.model || "",
      inputTokens: durable.inputTokens ?? null,
      outputTokens: durable.outputTokens ?? null,
      cacheReadTokens: durable.cacheReadTokens ?? null,
      cacheWriteTokens: durable.cacheWriteTokens ?? null,
      totalTokens: durable.totalTokens ?? null,
      costUsd: durable.costUsd ?? null,
      turnDurationMs: durable.turnDurationMs ?? null,
      source: durable.source || "unavailable",
      remoteCalls: durable.remoteCalls ?? null,
    };
  }
  const selected = logs.filter((entry) => entry.usage?.conversationId === conversationId);
  return latest(selected, (entry) => entry.usage);
}

function durableKnowledge(
  durable: TurnMetrics | null,
  logs: SessionLogEntry[],
  conversationId: string,
): SessionKnowledgeMetrics | null {
  const materialCount = durable?.materialCount;
  const corpusBytes = durable?.corpusBytes;
  const corpusUtf8Chars = durable?.corpusUtf8Chars;
  const corpusEstTokens = durable?.corpusEstTokens;
  // These four corpus facts are a single measured local snapshot. Do not turn
  // a malformed/older partial snapshot into invented zeroes; use the existing
  // compatibility fallback instead.
  if (
    durable &&
    materialCount != null &&
    corpusBytes != null &&
    corpusUtf8Chars != null &&
    corpusEstTokens != null
  ) {
    return {
      conversationId,
      materialCount,
      corpusBytes,
      corpusUtf8Chars,
      corpusEstTokens,
      retrievalCandidateCount: durable.retrievalCandidateCount ?? null,
      selectedEvidenceCount: durable.selectedEvidenceCount ?? null,
      selectedEvidenceBytes: durable.selectedEvidenceBytes ?? null,
      selectedEvidenceUtf8Chars: durable.selectedEvidenceUtf8Chars ?? null,
      evidenceEstTokens: durable.evidenceEstTokens ?? null,
      contextReductionPct: durable.contextReductionPct ?? null,
      semanticProviderState: durable.semanticProviderState || "",
      requestPreparationMs: durable.requestPreparationMs ?? null,
      retrievalMode: durable.retrievalMode ?? null,
      eligibleMaterials: durable.eligibleMaterials ?? null,
      materialsInspected: durable.materialsInspected ?? null,
      chunksInspected: durable.chunksInspected ?? null,
      exhaustiveCoverage: durable.exhaustiveCoverage ?? null,
      lexicalHits: durable.lexicalHits ?? null,
      semanticHits: durable.semanticHits ?? null,
      localMode: durable.localMode ?? null,
    };
  }
  // A partial durable record is authoritative but incomplete. Never fill it
  // from a session entry (which might belong to a different/in-flight turn).
  if (durable && Object.values(durable).some((value) => value != null)) return null;
  const selected = logs.filter((entry) => entry.knowledge?.conversationId === conversationId);
  return latest(selected, (entry) => entry.knowledge);
}
