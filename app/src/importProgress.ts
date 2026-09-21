import type { AcceptedImportProgressView } from "./types";

/**
 * A local, durable-state-only indication of embedding work. This is separate
 * from `materialsReady`: a material is ready only once every required chunk is
 * usable, while this ratio can advance throughout a large embedding pass.
 */
export function embeddingProgressPercent(accepted: AcceptedImportProgressView): number | null {
  if (accepted.embeddingsTotal <= 0) return null;
  const completed = Math.min(Math.max(accepted.embeddingCompleted, 0), accepted.embeddingsTotal);
  const percent = Math.round((completed * 100) / accepted.embeddingsTotal);
  // A non-terminal operation must never announce completion merely because
  // durable embedding work has reached its denominator.
  return accepted.state === "completed" ? Math.min(percent, 100) : Math.min(percent, 99);
}
