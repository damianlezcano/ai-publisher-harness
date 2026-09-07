import { useCallback, useEffect, useRef, useState } from "react";
import type { KeyboardEvent, RefObject } from "react";
import { api, errorMessage } from "../../api";
import type {
  ProviderSummary,
  SessionKnowledgeMetrics,
  SessionLogEntry,
  SessionUsage,
} from "../../types";
import ProviderCard from "./ProviderCard";
import Dialog from "../ui/Dialog";
import { messages } from "../../messages";

type SettingsTab = "general" | "logs";

const TAB_ORDER: SettingsTab[] = ["general", "logs"];

function latest<T>(
  logs: SessionLogEntry[],
  select: (entry: SessionLogEntry) => T | null | undefined,
): T | null {
  for (let index = logs.length - 1; index >= 0; index--) {
    const value = select(logs[index]);
    if (value) return value;
  }
  return null;
}

function tokenCount(value: number | null): string {
  return value == null
    ? "No informado por el proveedor"
    : `${value.toLocaleString("es-AR")} tokens`;
}

function cost(value: number | null): string {
  return value == null
    ? "No informado por el proveedor"
    : `USD ${value.toLocaleString("en-US", { maximumFractionDigits: 6 })}`;
}

function UsageSummary({ usage }: { usage: SessionUsage | null }) {
  return (
    <section className="session-usage-summary" aria-label="Último turno">
      <h4>Último turno</h4>
      <dl>
        <div>
          <dt>Entrada LLM</dt>
          <dd>{usage ? tokenCount(usage.inputTokens) : "No informado por el proveedor"}</dd>
        </div>
        <div>
          <dt>Salida LLM</dt>
          <dd>{usage ? tokenCount(usage.outputTokens) : "No informado por el proveedor"}</dd>
        </div>
        <div>
          <dt>Cache</dt>
          <dd>
            {usage
              ? `${tokenCount(usage.cacheReadTokens)} lectura · ${tokenCount(usage.cacheWriteTokens)} escritura`
              : "No informado por el proveedor"}
          </dd>
        </div>
        <div>
          <dt>Total</dt>
          <dd>{usage ? tokenCount(usage.totalTokens) : "No informado por el proveedor"}</dd>
        </div>
        <div>
          <dt>Costo informado</dt>
          <dd>{usage ? cost(usage.costUsd) : "No informado por el proveedor"}</dd>
        </div>
      </dl>
      {usage?.source === "estimated" && <p className="muted">Uso remoto estimado.</p>}
    </section>
  );
}

function KnowledgeSummary({ knowledge }: { knowledge: SessionKnowledgeMetrics | null }) {
  if (!knowledge) return null;
  return (
    <section className="session-usage-summary" aria-label="Knowledge">
      <h4>Knowledge</h4>
      <dl>
        <div>
          <dt>Corpus original estimado</dt>
          <dd>{knowledge.corpusEstTokens.toLocaleString("es-AR")} tokens</dd>
        </div>
        <div>
          <dt>Evidencia enviada estimada</dt>
          <dd>{knowledge.evidenceEstTokens.toLocaleString("es-AR")} tokens</dd>
        </div>
        <div>
          <dt>Reducción estimada de contexto</dt>
          <dd>{knowledge.contextReductionPct.toLocaleString("es-AR")} %</dd>
        </div>
      </dl>
      <p className="muted">Es una estimación de contexto, no un ahorro de facturación exacto.</p>
    </section>
  );
}

interface ProviderPanelProps {
  onClose: () => void;
  onChanged: () => void;
}

export default function ProviderPanel({ onClose, onChanged }: ProviderPanelProps) {
  const [tab, setTab] = useState<SettingsTab>("general");
  const [providers, setProviders] = useState<ProviderSummary[] | null>(null);
  const [loadingError, setLoadingError] = useState<string | null>(null);
  const [othersOpen, setOthersOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [logs, setLogs] = useState<SessionLogEntry[]>([]);
  const logsRef = useRef<HTMLPreElement>(null);
  const generalTabRef = useRef<HTMLButtonElement>(null);
  const logsTabRef = useRef<HTMLButtonElement>(null);

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
  const latestUsage = latest(logs, (entry) => entry.usage);
  const latestKnowledge = latest(logs, (entry) => entry.knowledge);

  const changed = () => {
    onChanged();
    void load();
  };

  async function refreshLogs() {
    try {
      const sessionLogs = await api.sessionLogs();
      setLogs(Array.isArray(sessionLogs) ? sessionLogs : []);
    } catch (err) {
      setLoadingError(errorMessage(err));
    }
  }

  async function clearLogs() {
    try {
      await api.sessionLogsClear();
      setLogs([]);
      setLoadingError(null);
    } catch (err) {
      setLoadingError(errorMessage(err));
    }
  }

  useEffect(() => {
    const node = logsRef.current;
    if (node && typeof node.scrollTo === "function") node.scrollTo({ top: node.scrollHeight });
  }, [logs]);

  async function copyLogs() {
    try {
      await navigator.clipboard?.writeText(
        logs.map((entry) => `[${entry.level}] ${entry.message}`).join("\n"),
      );
    } catch (err) {
      setLoadingError(errorMessage(err));
    }
  }

  function selectTab(next: SettingsTab) {
    setTab(next);
    if (next === "general") generalTabRef.current?.focus();
    else logsTabRef.current?.focus();
  }

  function onTabsKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    const index = TAB_ORDER.indexOf(tab);
    const delta = event.key === "ArrowRight" ? 1 : -1;
    selectTab(TAB_ORDER[(index + delta + TAB_ORDER.length) % TAB_ORDER.length]);
  }

  return (
    <Dialog
      title={messages.provider.heading}
      onClose={onClose}
      className="provider-dialog"
      closeButton
      initialFocusRef={generalTabRef as RefObject<HTMLElement>}
    >
      <div
        className="settings-tabs"
        role="tablist"
        aria-label={messages.provider.panelLabel}
        onKeyDown={onTabsKeyDown}
      >
        <button
          type="button"
          id="settings-tab-general"
          role="tab"
          aria-selected={tab === "general"}
          aria-controls="settings-panel-general"
          tabIndex={tab === "general" ? 0 : -1}
          ref={generalTabRef}
          onClick={() => setTab("general")}
          className={`settings-tab${tab === "general" ? " active" : ""}`}
        >
          {messages.provider.tabs.general}
        </button>
        <button
          type="button"
          id="settings-tab-logs"
          role="tab"
          aria-selected={tab === "logs"}
          aria-controls="settings-panel-logs"
          tabIndex={tab === "logs" ? 0 : -1}
          ref={logsTabRef}
          onClick={() => setTab("logs")}
          className={`settings-tab${tab === "logs" ? " active" : ""}`}
        >
          {messages.provider.tabs.logs}
        </button>
      </div>

      <div
        id="settings-panel-general"
        role="tabpanel"
        aria-labelledby="settings-tab-general"
        hidden={tab !== "general"}
        className="settings-panel"
      >
        <p className="muted">{messages.provider.privacyNote}</p>

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
      </div>

      <div
        id="settings-panel-logs"
        role="tabpanel"
        aria-labelledby="settings-tab-logs"
        hidden={tab !== "logs"}
        className="settings-panel"
      >
        <section className="provider-section" aria-label={messages.sessionLogs.heading}>
          <h3>{messages.sessionLogs.heading}</h3>
          <p className="muted">{messages.sessionLogs.description}</p>
          <UsageSummary usage={latestUsage} />
          <KnowledgeSummary knowledge={latestKnowledge} />
          <div className="row-actions">
            <button type="button" className="secondary" onClick={() => void clearLogs()}>
              {messages.sessionLogs.clear}
            </button>
            <button type="button" className="secondary" onClick={() => void refreshLogs()}>
              {messages.sessionLogs.refresh}
            </button>
            <button
              type="button"
              className="secondary"
              onClick={() => void copyLogs()}
              disabled={logs.length === 0}
            >
              {messages.sessionLogs.copy}
            </button>
          </div>
          <p className="sr-only" aria-live="polite" aria-atomic="true">
            {logs.length > 0
              ? messages.sessionLogs.latestAnnouncement(logs[logs.length - 1].level)
              : ""}
          </p>
          <pre ref={logsRef} className="session-logs">
            {logs.length === 0
              ? messages.sessionLogs.empty
              : logs.map((entry) => `[${entry.level}] ${entry.message}`).join("\n")}
          </pre>
        </section>
        {loadingError && (
          <p className="error" role="alert">
            {loadingError}
          </p>
        )}
      </div>
    </Dialog>
  );
}
