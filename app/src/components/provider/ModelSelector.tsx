import { useEffect, useState } from "react";
import { api, errorMessage } from "../../api";
import type { ModelSummary, ProviderSummary, SelectedModelView } from "../../types";
import { messages } from "../../messages";
import { modelOptionLabel } from "../../labels";

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
    return [
      ...add(messages.model.groupRecommended, recommended),
      ...add(messages.model.groupFree, free),
      ...connectedGroups,
    ];
  })();

  const currentValue = selected?.model
    ? `${selected.model.providerId}::${selected.model.modelId}`
    : "";

  const valueInGroups = groups.some((g) => g.options.some((o) => o.value === currentValue));

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
        {messages.model.label}
      </label>
      {selected?.requiresChoice && (
        <p className="notice" role="alert">
          {selected.notice ?? messages.model.unavailableChoice}
        </p>
      )}
      <select
        id="model-select"
        value={valueInGroups ? currentValue : ""}
        onChange={(e) => void change(e.target.value)}
        disabled={loading}
      >
        {loading && <option value="">{messages.model.loading}</option>}
        {!loading && groups.length === 0 && <option value="">{messages.model.none}</option>}
        {!loading && groups.length > 0 && !valueInGroups && (
          <option value="" disabled>
            {messages.model.choose}
          </option>
        )}
        {groups.map((group) => (
          <optgroup key={group.label} label={group.label}>
            {group.options.map((option) => (
              <option key={option.value} value={option.value}>
                {modelOptionLabel(option.model)}
              </option>
            ))}
          </optgroup>
        ))}
      </select>
      {selected?.model && !selected.requiresChoice && (
        <span className={`model-badge ${selected.model.free ? "free" : "paid"}`}>
          {selected.model.free ? messages.model.free : messages.model.paid}
        </span>
      )}
      {models.some((m) => m.free) && <p className="notice">{messages.model.freeModelsNotice}</p>}
      {selected?.notice && !selected.requiresChoice && <p className="notice">{selected.notice}</p>}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
