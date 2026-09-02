import { useCallback, useEffect, useState } from "react";
import { api, errorMessage } from "../../api";
import type { ProviderSummary } from "../../types";
import ProviderCard from "./ProviderCard";
import ModelSelector from "./ModelSelector";
import Dialog from "../ui/Dialog";
import { messages } from "../../messages";

interface ProviderPanelProps {
  onClose: () => void;
  onChanged: () => void;
}

export default function ProviderPanel({ onClose, onChanged }: ProviderPanelProps) {
  const [providers, setProviders] = useState<ProviderSummary[] | null>(null);
  const [loadingError, setLoadingError] = useState<string | null>(null);
  const [othersOpen, setOthersOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [modelRefreshKey, setModelRefreshKey] = useState(0);

  const load = useCallback(async () => {
    try {
      const list = await api.providerList();
      setProviders(list);
      setLoadingError(null);
    } catch (err) {
      setLoadingError(errorMessage(err));
    }
  }, []);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const list = await api.providerList();
        if (active) {
          setProviders(list);
          setLoadingError(null);
        }
      } catch (err) {
        if (active) setLoadingError(errorMessage(err));
      }
    })();
    return () => {
      active = false;
    };
  }, [load]);

  const featured = (providers ?? []).filter((p) => p.highlighted);
  const others = (providers ?? []).filter((p) => !p.highlighted);
  const filteredOthers = others.filter((p) =>
    p.name.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()),
  );

  const changed = () => {
    onChanged();
    setModelRefreshKey((k) => k + 1);
    void load();
  };

  return (
    <Dialog
      title={messages.provider.heading}
      onClose={onClose}
      className="provider-dialog"
      closeButton
    >
      <p className="muted">{messages.provider.privacyNote}</p>

      <ModelSelector refreshKey={modelRefreshKey} />

      {loadingError && (
        <p className="error" role="alert">
          {loadingError}
        </p>
      )}

      {providers === null && !loadingError && <p className="muted">{messages.app.loading}</p>}

      {providers && (
        <div className="provider-list">
          <section className="provider-section">
            <h3>{messages.provider.featuredHeading}</h3>
            {featured.length === 0 && <p className="muted">{messages.provider.noFeatured}</p>}
            {featured.map((provider) => (
              <ProviderCard key={provider.id} provider={provider} onChanged={changed} />
            ))}
          </section>

          <section className="provider-section">
            <button
              type="button"
              className="secondary"
              aria-expanded={othersOpen}
              onClick={() => setOthersOpen((v) => !v)}
            >
              {messages.provider.othersButton(others.length)}
            </button>
            {othersOpen && (
              <div className="provider-others">
                <input
                  type="search"
                  value={query}
                  onChange={(e) => setQuery(e.target.value)}
                  placeholder={messages.provider.searchPlaceholder}
                  aria-label={messages.provider.searchAriaLabel}
                />
                {filteredOthers.length === 0 && (
                  <p className="muted">{messages.provider.noSearchResults}</p>
                )}
                {filteredOthers.map((provider) => (
                  <ProviderCard key={provider.id} provider={provider} onChanged={changed} />
                ))}
              </div>
            )}
          </section>
        </div>
      )}
    </Dialog>
  );
}
