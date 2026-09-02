import type { ModelSummary } from "./types";
import { messages } from "./messages";

export { kindLabel, kindIcon, visibilityLabel, humanSize, humanDate } from "./messages";

export function modelOptionLabel(model: ModelSummary): string {
  const nameLooksLikeId = model.name === model.modelId;
  if (nameLooksLikeId) {
    if (model.free) {
      return model.recommended ? messages.model.automaticFree : messages.model.free;
    }
    return messages.model.paid;
  }
  return `${model.name}${model.free ? messages.model.freeSuffix : messages.model.paidSuffix}`;
}
