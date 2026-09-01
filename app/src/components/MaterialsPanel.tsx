import { api } from "../api";
import { kindIcon } from "../labels";
import { messages } from "../messages";
import type { MaterialView } from "../types";

interface MaterialChipProps {
  projectId: string;
  material: MaterialView;
}

export function MaterialChip({ projectId, material }: MaterialChipProps) {
  async function open() {
    try {
      await api.materialOpen(projectId, material.id);
    } catch {
      // Intentionally silent: the chip is a convenience open action.
    }
  }

  return (
    <span className="attachment-chip">
      <span className="attachment-icon" aria-hidden="true">
        {kindIcon(material.kind)}
      </span>
      <span className="attachment-name">{material.displayName}</span>
      <button
        type="button"
        className="ghost attachment-open"
        onClick={() => void open()}
        aria-label={`${messages.common.open} ${material.displayName}`}
      >
        {messages.common.open}
      </button>
    </span>
  );
}
