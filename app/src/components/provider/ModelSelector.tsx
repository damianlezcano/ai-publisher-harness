import { useEffect, useState } from "react";
import { api, errorMessage } from "../../api";
import type { ModelSummary, ProviderSummary, SelectedModelView } from "../../types";

interface ModelSelectorProps {
  /** Bumped by the parent after provider/model mutations so the list reloads. */
  refreshKey: number;
}

interface Group {
  label: string;
  options: Array<{ value: string; model: ModelSummary }>;
}

export default function ModelSelector({ refreshKey }: ModelSelectorProps) {
  const [selected, setSelected] = useState<SelectedModelView | null>(null);
  const [models, setModels] = useState<ModelSummary[]>([]);
  const [providers, setProviders] = useState<ProviderSummary[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const [modelList, providerList, current] = await Promise.all([
          api.modelList(),
          api.providerList(),
          api.modelGetSelected(),
        ]);
        if (!active) return;
        setModels(modelList);
        setProviders(providerList);
        setSelected(current);
      } catch (err) {
        if (active) setError(errorMessage(err));
      } finally {
        if (active) setLoading(false);
      }
    })();
    return () => {
      active = false;
    };
  }, [refreshKey]);

  const connectedIds = new Set(providers.filter((p) => p.connected).map((p) => p.id));
  const visible = (m: ModelSummary) => m.free || connectedIds.has(m.providerId);

  const groups: Group[] = (() => {
    const seen = new Set<string>();
    const add = (label: string, items: ModelSummary[]) => {
      const options = items
        .filter((m) => visible(m) && !seen.has(`${m.providerId}::${m.modelId}`))
        .map((m) => {
          seen.add(`${m.providerId}::${m.modelId}`);
          return { value: `${m.providerId}::${m.modelId}`, model: m };
        });
      return options.length > 0 ? [{ label, options }] : [];
    };
    const recommended = models.filter((m) => m.recommended);
    const free = models.filter((m) => m.free);
    const connectedGroups: Group[] = providers
      .filter((p) => p.connected)
      .map((p) => ({
        label: p.name,
        options: models
          .filter((m) => m.providerId === p.id)
          .filter((m) => !seen.has(`${m.providerId}::${m.modelId}`))
          .map((m) => {
            seen.add(`${m.providerId}::${m.modelId}`);
            return { value: `${m.providerId}::${m.modelId}`, model: m };
          }),
      }))
      .filter((g) => g.options.length > 0);
    return [...add("Recomendado", recommended), ...add("Gratis", free), ...connectedGroups];
  })();

  const currentValue = selected?.model
    ? `${selected.model.providerId}::${selected.model.modelId}`
    : "";

  async function change(next: string) {
    if (next === "") return;
    const [providerId, modelId] = next.split("::");
    try {
      await api.modelSelect(providerId, modelId);
      const current = await api.modelGetSelected();
      setSelected(current);
      setError(null);
    } catch (err) {
      setError(errorMessage(err));
    }
  }

  return (
    <div className="model-selector">
      <label className="model-label" htmlFor="model-select">
        Modelo
      </label>
      {selected?.requiresChoice && (
        <p className="notice" role="alert">
          {selected.notice ?? "Este modelo ya no está disponible. Elegí otro."}
        </p>
      )}
      <select
        id="model-select"
        value={
          groups.some((g) => g.options.some((o) => o.value === currentValue)) ? currentValue : ""
        }
        onChange={(e) => void change(e.target.value)}
        disabled={loading}
      >
        {loading && <option value="">Cargando…</option>}
        {!loading && groups.length === 0 && <option value="">Sin modelos</option>}
        {groups.map((group) => (
          <optgroup key={group.label} label={group.label}>
            {group.options.map((option) => (
              <option key={option.value} value={option.value}>
                {option.model.name}
              </option>
            ))}
          </optgroup>
        ))}
      </select>
      {selected?.model && !selected.requiresChoice && (
        <span className={`model-badge ${selected.model.free ? "free" : "paid"}`}>
          {selected.model.free ? "Gratis" : "De pago"}
        </span>
      )}
      {selected?.notice && !selected.requiresChoice && <p className="notice">{selected.notice}</p>}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
