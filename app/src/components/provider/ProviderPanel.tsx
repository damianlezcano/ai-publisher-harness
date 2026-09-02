import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorMessage } from "../../api";
import type { ProviderSummary, SessionLogEntry } from "../../types";
import ProviderCard from "./ProviderCard";
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
  const [logs, setLogs] = useState<SessionLogEntry[]>([]);
  const logsRef = useRef<HTMLPreElement>(null);

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
        const sessionLogs = await api.sessionLogs().catch(() => []);
        if (active) {
          setProviders(list);
          setLogs(Array.isArray(sessionLogs) ? sessionLogs : []);
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

  async function clearLogs() {
    await api.sessionLogsClear();
    setLogs([]);
  }

  useEffect(() => {
    const node = logsRef.current;
    if (node && typeof node.scrollTo === "function") node.scrollTo({ top: node.scrollHeight });
  }, [logs]);

  async function copyLogs() {
    await navigator.clipboard?.writeText(
      logs.map((entry) => `[${entry.level}] ${entry.message}`).join("\n"),
    );
  }

  return (
    <Dialog
      title={messages.provider.heading}
      onClose={onClose}
      className="provider-dialog"
      closeButton
    >
      <p className="muted">{messages.provider.privacyNote}</p>

      <section className="provider-section" aria-label="Logs de esta sesión">
        <h3>Logs de esta sesión</h3>
        <p className="muted">
          Información de EducAI durante esta ejecución. No incluye contenido de tus archivos ni
          mensajes.
        </p>
        <div className="row-actions">
          <button type="button" className="secondary" onClick={() => void clearLogs()}>
            Limpiar
          </button>
          <button
            type="button"
            className="secondary"
            onClick={() => void copyLogs()}
            disabled={logs.length === 0}
          >
            Copiar
          </button>
        </div>
        <pre ref={logsRef} className="session-logs" aria-live="polite">
          {logs.length === 0
            ? "Sin eventos todavía."
            : logs.map((entry) => `[${entry.level}] ${entry.message}`).join("\n")}
        </pre>
      </section>

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
