import { api } from "../api";
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
    <button
      type="button"
      className="chip"
      onClick={() => void open()}
      aria-label={`${messages.common.open} ${material.displayName}`}
    >
      {material.displayName}
    </button>
  );
}
