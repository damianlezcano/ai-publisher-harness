import { useEffect, useRef, useState } from "react";
import { api, errorMessage } from "../../api";
import type { OAuthAttempt, OAuthStatusKind, ProviderDetail, ProviderSummary } from "../../types";

interface ProviderCardProps {
  provider: ProviderSummary;
  onChanged: () => void;
}

const POLL_INTERVAL_MS = 2000;

export default function ProviderCard({ provider, onChanged }: ProviderCardProps) {
  const [detail, setDetail] = useState<ProviderDetail | null>(null);
  const [keyInput, setKeyInput] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<string | null>(null);
  const [oauth, setOauth] = useState<{
    attempt: OAuthAttempt;
    status: OAuthStatusKind;
    codeInput: string;
  } | null>(null);
  const [expanded, setExpanded] = useState(false);

  const pollTimer = useRef<ReturnType<typeof setInterval> | null>(null);

  useEffect(() => {
    let active = true;
    void (async () => {
      try {
        const loaded = await api.providerDetail(provider.id);
        if (active) setDetail(loaded);
      } catch {
        // The card still works from the summary; detail is only for connections.
      }
    })();
    return () => {
      active = false;
    };
  }, [provider.id]);

  useEffect(() => {
    if (oauth?.status !== "pending") return;
    pollTimer.current = setInterval(() => {
      void (async () => {
        try {
          const status = await api.providerOauthStatus(oauth.attempt.attemptId);
          if (status.status === "complete") {
            setOauth((prev) => (prev ? { ...prev, status: "complete" } : prev));
            onChanged();
          } else if (status.status === "failed" || status.status === "expired") {
            setOauth((prev) => (prev ? { ...prev, status: status.status } : prev));
            setError("No pudimos completar la conexión. Intentalo de nuevo.");
          }
        } catch {
          // Keep polling; transient failures are expected.
        }
      })();
    }, POLL_INTERVAL_MS);
    return () => {
      if (pollTimer.current) clearInterval(pollTimer.current);
    };
  }, [oauth?.attempt.attemptId, oauth?.status, onChanged]);

  async function connectKey() {
    const key = keyInput.trim();
    if (key === "") return;
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      await api.providerConnectKey(provider.id, key);
      setKeyInput("");
      setNotice("Conectado.");
      onChanged();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function beginOauth(methodId: string) {
    setBusy(true);
    setError(null);
    try {
      const attempt = await api.providerOauthBegin(provider.id, methodId);
      setOauth({ attempt, status: "pending", codeInput: "" });
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function completeOauth() {
    if (!oauth) return;
    setBusy(true);
    setError(null);
    try {
      await api.providerOauthComplete(
        oauth.attempt.attemptId,
        oauth.attempt.mode === "code" ? oauth.codeInput : null,
      );
      setOauth(null);
      setNotice("Cuenta conectada.");
      onChanged();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function cancelOauth() {
    if (!oauth) return;
    setBusy(true);
    try {
      await api.providerOauthCancel(oauth.attempt.attemptId);
    } catch {
      // Cancel is best-effort; the local state is dropped regardless.
    }
    setOauth(null);
    setBusy(false);
  }

  async function disconnect() {
    const connection = detail?.connections[0];
    if (!connection) return;
    setBusy(true);
    setError(null);
    try {
      await api.providerDisconnect(connection.id);
      setNotice("Desconectado.");
      onChanged();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function testConnection() {
    setTesting(true);
    setTestResult(null);
    setError(null);
    try {
      const result = await api.providerTestConnection(provider.id);
      setTestResult(result.message);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setTesting(false);
    }
  }

  const connection = detail?.connections[0];

  return (
    <article className={`provider-card${provider.connected ? " connected" : ""}`}>
      <div className="provider-head">
        <div className="provider-name">
          <strong>{provider.name}</strong>
          {provider.connected && (
            <span className="provider-state ok">{connection?.label ?? "Conectado"}</span>
          )}
        </div>
        <button
          type="button"
          className="secondary"
          onClick={() => setExpanded((v) => !v)}
          aria-expanded={expanded}
        >
          {expanded ? "Ocultar" : provider.connected ? "Configurar" : "Conectar"}
        </button>
      </div>

      {expanded && (
        <div className="provider-body">
          {provider.authMethods.map((method) =>
            method.kind === "api_key" ? (
              <form
                key="api_key"
                className="inline-form"
                onSubmit={(e) => {
                  e.preventDefault();
                  void connectKey();
                }}
              >
                <label className="sr-only" htmlFor={`key-${provider.id}`}>
                  {method.label}
                </label>
                <input
                  id={`key-${provider.id}`}
                  type="password"
                  autoComplete="off"
                  value={keyInput}
                  onChange={(e) => setKeyInput(e.target.value)}
                  placeholder={method.label}
                  disabled={busy}
                />
                <button type="submit" className="primary" disabled={busy || keyInput.trim() === ""}>
                  Conectar
                </button>
              </form>
            ) : (
              <div key={method.methodId ?? method.label} className="provider-method">
                {!oauth && (
                  <button
                    type="button"
                    className="secondary"
                    disabled={busy}
                    onClick={() => method.methodId && void beginOauth(method.methodId)}
                  >
                    {method.label}
                  </button>
                )}
                {oauth && (
                  <div className="oauth-box">
                    <p className="muted">
                      {oauth.attempt.instructions ?? "Abrí el enlace y aprobá el acceso."}
                    </p>
                    {oauth.status === "pending" && (
                      <>
                        <button
                          type="button"
                          className="primary"
                          onClick={() =>
                            void api
                              .providerOauthOpen(oauth.attempt.url)
                              .catch((err) => setError(errorMessage(err)))
                          }
                        >
                          Abrir en el navegador
                        </button>
                        {oauth.attempt.mode === "code" && (
                          <input
                            type="text"
                            value={oauth.codeInput}
                            onChange={(e) =>
                              setOauth((prev) =>
                                prev ? { ...prev, codeInput: e.target.value } : prev,
                              )
                            }
                            placeholder="Código de verificación"
                          />
                        )}
                        <div className="row-actions">
                          <button
                            type="button"
                            className="primary"
                            onClick={() => void completeOauth()}
                          >
                            Completar
                          </button>
                          <button
                            type="button"
                            className="secondary"
                            onClick={() => void cancelOauth()}
                          >
                            Cancelar
                          </button>
                        </div>
                      </>
                    )}
                    {oauth.status === "complete" && <p className="status published">Conectado.</p>}
                  </div>
                )}
              </div>
            ),
          )}

          <div className="row-actions wrap">
            <button
              type="button"
              className="secondary"
              disabled={testing || busy}
              onClick={() => void testConnection()}
            >
              {testing ? "Probando…" : "Probar conexión"}
            </button>
            {connection && (
              <button
                type="button"
                className="danger"
                disabled={busy}
                onClick={() => void disconnect()}
              >
                Desconectar
              </button>
            )}
          </div>

          {testResult && <p className="status published">{testResult}</p>}
          {notice && <p className="status published">{notice}</p>}
          {error && (
            <p className="error" role="alert">
              {error}
            </p>
          )}
        </div>
      )}
    </article>
  );
}
