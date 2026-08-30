import { useCallback, useEffect, useState } from "react";
import { api, errorMessage } from "../../api";
import type { ProviderSummary } from "../../types";
import ProviderCard from "./ProviderCard";

interface ProviderPanelProps {
  onClose: () => void;
  onChanged: () => void;
}

export default function ProviderPanel({ onClose, onChanged }: ProviderPanelProps) {
  const [providers, setProviders] = useState<ProviderSummary[] | null>(null);
  const [loadingError, setLoadingError] = useState<string | null>(null);
  const [othersOpen, setOthersOpen] = useState(false);
  const [query, setQuery] = useState("");

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
    void load();
  };

  return (
    <div className="dialog-backdrop" role="presentation">
      <div
        className="dialog provider-dialog"
        role="dialog"
        aria-modal="true"
        aria-label="Conectá tu IA"
      >
        <header className="provider-panel-header">
          <h2>Conectá tu IA</h2>
          <button type="button" className="secondary" onClick={onClose}>
            Cerrar
          </button>
        </header>
        <p className="muted">
          Tu cuenta y tus claves se guardan de forma segura en tu computadora. Nunca se comparten.
        </p>

        {loadingError && (
          <p className="error" role="alert">
            {loadingError}
          </p>
        )}

        {providers === null && !loadingError && <p className="muted">Cargando…</p>}

        {providers && (
          <div className="provider-list">
            <section className="provider-section">
              <h3>Recomendados</h3>
              {featured.length === 0 && <p className="muted">Aún no hay proveedores destacados.</p>}
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
                Otros proveedores ({others.length})
              </button>
              {othersOpen && (
                <div className="provider-others">
                  <input
                    type="search"
                    value={query}
                    onChange={(e) => setQuery(e.target.value)}
                    placeholder="Buscar proveedor"
                    aria-label="Buscar proveedor"
                  />
                  {filteredOthers.length === 0 && (
                    <p className="muted">No encontramos proveedores.</p>
                  )}
                  {filteredOthers.map((provider) => (
                    <ProviderCard key={provider.id} provider={provider} onChanged={changed} />
                  ))}
                </div>
              )}
            </section>
          </div>
        )}
      </div>
    </div>
  );
}
