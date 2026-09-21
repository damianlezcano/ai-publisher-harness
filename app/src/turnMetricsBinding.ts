import type { MessageView, TurnMetrics } from "./types";

/**
 * Maps each assistant message to the durable per-turn metrics of the owning
 * user message.
 *
 * Binding is keyed by the persisted assistant `turnId` (which equals the user
 * message id — the turn id). This is the durable assistant↔turn identity, so
 * a failed assistant followed by a recovered assistant can never steal metrics
 * from the real answer.
 *
 * Legacy records written before assistant `turnId` existed fall back to a
 * conservative positional rule: metrics are attributed only when a user turn is
 * followed by exactly one assistant response. A fail→recover ambiguity yields
 * no metrics rather than wrong metrics.
 */
export function turnMetricsByAssistantId(messageList: MessageView[]): Map<string, TurnMetrics> {
  const ordered = [...messageList].sort((a, b) => {
    const byTime = a.createdAt.localeCompare(b.createdAt);
    return byTime !== 0 ? byTime : a.id.localeCompare(b.id);
  });

  const metricsByUserId = new Map<string, TurnMetrics>();
  for (const message of ordered) {
    if (message.role === "user" && message.turnMetrics) {
      metricsByUserId.set(message.id, message.turnMetrics);
    }
  }

  const map = new Map<string, TurnMetrics>();

  // Durable binding: an assistant references its owning user turn id directly.
  // Failed/cancelled assistants never render metrics, so they bind nothing.
  for (const message of ordered) {
    if (message.role !== "assistant") continue;
    if (message.status !== "ok") continue;
    if (message.turnId == null) continue;
    const metrics = metricsByUserId.get(message.turnId);
    if (metrics) map.set(message.id, metrics);
  }

  // Conservative legacy fallback. A user turn followed by exactly one assistant
  // is unambiguous; anything else (including fail→recover) binds nothing.
  for (let i = 0; i < ordered.length; i++) {
    const message = ordered[i];
    if (message.role !== "user") continue;
    const group: MessageView[] = [];
    for (let j = i + 1; j < ordered.length && ordered[j].role === "assistant"; j++) {
      group.push(ordered[j]);
    }
    if (group.length === 1 && group[0].turnId == null && group[0].status === "ok") {
      const metrics = metricsByUserId.get(message.id);
      if (metrics && !map.has(group[0].id)) map.set(group[0].id, metrics);
    }
  }

  return map;
}
