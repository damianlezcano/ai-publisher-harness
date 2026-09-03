import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import ProviderPanel from "./ProviderPanel";
import type { ProviderDetail, ProviderSummary } from "../../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

const openaiSummary: ProviderSummary = {
  id: "openai",
  name: "ChatGPT",
  authMethods: [{ kind: "api_key", methodId: null, label: "Clave de acceso", prompts: [] }],
  connected: false,
  connectionLabel: null,
  highlighted: true,
};

const openaiWithOauth: ProviderSummary = {
  ...openaiSummary,
  authMethods: [
    { kind: "api_key", methodId: null, label: "Clave de acceso", prompts: [] },
    {
      kind: "account",
      methodId: "chatgpt-browser",
      label: "Conectá tu cuenta",
      prompts: [],
    },
  ],
};

const googleSummary: ProviderSummary = {
  id: "google",
  name: "Gemini",
  authMethods: [{ kind: "api_key", methodId: null, label: "Clave de acceso", prompts: [] }],
  connected: true,
  connectionLabel: "mi clave",
  highlighted: false,
};

const detail: ProviderDetail = {
  id: "openai",
  name: "ChatGPT",
  authMethods: openaiSummary.authMethods,
  connections: [],
};

function modelCommands(cmd: string): Promise<unknown> | undefined {
  if (cmd === "model_get_selected") {
    return Promise.resolve({
      model: {
        providerId: "opencode",
        modelId: "big-pickle",
        name: "big-pickle",
        free: true,
        recommended: true,
        deprecated: false,
      },
      notice: null,
      requiresChoice: false,
    });
  }
  if (cmd === "model_list") {
    return Promise.resolve([]);
  }
  return undefined;
}

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockImplementation((cmd: string) => {
    const models = modelCommands(cmd);
    if (models) return models;
    if (cmd === "provider_list") {
      return Promise.resolve([openaiSummary, googleSummary]);
    }
    if (cmd === "provider_detail") {
      return Promise.resolve(detail);
    }
    if (cmd === "provider_connect_key") {
      return Promise.resolve({ id: "cred-1", label: null });
    }
    if (cmd === "provider_test_connection") {
      return Promise.resolve({ outcome: "connected", message: "Conectado." });
    }
    return Promise.reject(new Error(`unexpected invoke ${cmd}`));
  });
});

describe("ProviderPanel", () => {
  it("opens with the General tab selected and logs never visible by default", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "provider_list") return Promise.resolve([]);
      if (cmd === "session_logs")
        return Promise.resolve([{ level: "INFO", message: "turn started" }]);
      return Promise.resolve(undefined);
    });
    render(<ProviderPanel onClose={() => {}} onChanged={() => {}} />);
    const dialog = await screen.findByRole("dialog", { name: "Configuración" });
    expect(dialog).toHaveClass("provider-dialog");

    const generalTab = screen.getByRole("tab", { name: "General" });
    const logsTab = screen.getByRole("tab", { name: "Logs" });
    expect(generalTab).toHaveAttribute("aria-selected", "true");
    expect(logsTab).toHaveAttribute("aria-selected", "false");
    expect(generalTab).toHaveFocus();

    // Logs content is not visible until the user explicitly selects Logs.
    const logsPanel = document.getElementById("settings-panel-logs");
    expect(logsPanel).toHaveAttribute("role", "tabpanel");
    expect(logsPanel).toHaveAttribute("aria-labelledby", "settings-tab-logs");
    expect(logsPanel).not.toBeVisible();
    expect(screen.getByText("Recomendados")).toBeVisible();
  });

  it("requires an explicit click on Logs to reveal the current-session viewer and clears it", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "provider_list") return Promise.resolve([]);
      if (cmd === "session_logs")
        return Promise.resolve([{ level: "INFO", message: "turn started" }]);
      if (cmd === "session_logs_clear") return Promise.resolve(undefined);
      return Promise.resolve(undefined);
    });
    render(<ProviderPanel onClose={() => {}} onChanged={() => {}} />);
    await screen.findByRole("dialog", { name: "Configuración" });
    expect(screen.queryByText("[INFO] turn started")).not.toBeVisible();

    await userEvent.click(screen.getByRole("tab", { name: "Logs" }));
    expect(screen.getByRole("tab", { name: "Logs" })).toHaveAttribute("aria-selected", "true");
    const logs = await screen.findByText("[INFO] turn started");
    expect(logs.closest("pre")).toHaveClass("session-logs");
    expect(logs).toBeVisible();
    await userEvent.click(screen.getByRole("button", { name: "Limpiar" }));
    expect(invokeMock).toHaveBeenCalledWith("session_logs_clear");
    expect(screen.getByText("Sin eventos todavía.")).toBeInTheDocument();
  });

  it("keeps Configuración open and focused when switching tabs, and supports arrow keys", async () => {
    const onClose = vi.fn();
    render(<ProviderPanel onClose={onClose} onChanged={() => {}} />);
    await screen.findByRole("dialog", { name: "Configuración" });

    await userEvent.click(screen.getByRole("tab", { name: "Logs" }));
    expect(screen.getByRole("tab", { name: "Logs" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Logs" })).toHaveFocus();
    expect(screen.getByRole("dialog", { name: "Configuración" })).toBeInTheDocument();

    await userEvent.keyboard("{ArrowLeft}");
    expect(screen.getByRole("tab", { name: "General" })).toHaveFocus();
    expect(screen.getByRole("tab", { name: "General" })).toHaveAttribute("aria-selected", "true");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("traps focus to visible controls, never wrapping into the hidden Logs panel", async () => {
    render(<ProviderPanel onClose={() => {}} onChanged={() => {}} />);
    const dialog = await screen.findByRole("dialog", { name: "Configuración" });

    // The last visible focusable in the General panel is the "Otros
    // proveedores" toggle; the Logs panel buttons are mounted but [hidden].
    const others = screen.getByRole("button", { name: "Otros proveedores (1)" });
    others.focus();
    expect(others).toHaveFocus();

    await userEvent.tab();

    const active = document.activeElement as HTMLElement;
    expect(active).not.toBe(document.body);
    expect(dialog.contains(active)).toBe(true);
    expect(active.closest("[hidden]")).toBeNull();
    // The trap wraps to the first visible control (the header close button),
    // never to the hidden Logs panel buttons or out of the dialog.
    expect(screen.getByRole("button", { name: "Cerrar" })).toHaveFocus();
  });

  it("always reopens on the General tab, never remembering Logs", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "provider_list") return Promise.resolve([]);
      if (cmd === "session_logs") return Promise.resolve([]);
      return Promise.resolve(undefined);
    });
    const { unmount } = render(<ProviderPanel onClose={() => {}} onChanged={() => {}} />);
    await screen.findByRole("dialog", { name: "Configuración" });
    await userEvent.click(screen.getByRole("tab", { name: "Logs" }));
    expect(screen.getByRole("tab", { name: "Logs" })).toHaveAttribute("aria-selected", "true");
    unmount();

    render(<ProviderPanel onClose={() => {}} onChanged={() => {}} />);
    await screen.findByRole("dialog", { name: "Configuración" });
    expect(screen.getByRole("tab", { name: "General" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Logs" })).toHaveAttribute("aria-selected", "false");
    expect(document.getElementById("settings-panel-logs")).not.toBeVisible();
  });

  it("renders as a labelled dialog and closes on Escape", async () => {
    const onClose = vi.fn();
    render(<ProviderPanel onClose={onClose} onChanged={() => {}} />);
    expect(await screen.findByRole("dialog", { name: "Configuración" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "General" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Logs" })).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("keeps the Configuración -> Logs de esta sesión contract visible and bounded behind an explicit click", async () => {
    const refreshLogs = vi.fn();
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "provider_list") return Promise.resolve([]);
      if (cmd === "session_logs") {
        refreshLogs();
        return Promise.resolve([
          { level: "INFO", message: "startup version=0.1.0" },
          { level: "WARN", message: "conversation unavailable falling_back=global" },
          { level: "ERROR", message: "turn failed conversation_id=x" },
        ]);
      }
      if (cmd === "session_logs_clear") return Promise.resolve(undefined);
      return Promise.resolve(undefined);
    });
    render(<ProviderPanel onClose={() => {}} onChanged={() => {}} />);
    const dialog = await screen.findByRole("dialog", { name: "Configuración" });

    // The logs live in their own tab panel and are hidden by default.
    await userEvent.click(screen.getByRole("tab", { name: "Logs" }));
    const heading = screen.getByRole("heading", { name: "Logs de esta sesión" });
    const section = heading.closest("section");
    expect(section).toHaveAttribute("aria-label", "Logs de esta sesión");
    expect(dialog.querySelector("section[aria-label='Logs de esta sesión']")).not.toBeNull();

    // Bounded in-memory viewer with the established actions.
    for (const action of ["Limpiar", "Actualizar", "Copiar"]) {
      expect(screen.getByRole("button", { name: action })).toBeInTheDocument();
    }
    const pre = screen.getByText(/turn failed conversation_id=x/).closest("pre");
    expect(pre).toHaveClass("session-logs");

    // Actualizar re-reads the current process buffer (never previous sessions).
    await userEvent.click(screen.getByRole("button", { name: "Actualizar" }));
    expect(refreshLogs).toHaveBeenCalled();
  });

  it("shows the ephemeral empty state when the current process has no events", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "provider_list") return Promise.resolve([]);
      if (cmd === "session_logs") return Promise.resolve([]);
      return Promise.resolve(undefined);
    });
    render(<ProviderPanel onClose={() => {}} onChanged={() => {}} />);
    await screen.findByRole("dialog", { name: "Configuración" });
    await userEvent.click(screen.getByRole("tab", { name: "Logs" }));
    expect(await screen.findByText("Logs de esta sesión")).toBeInTheDocument();
    expect(screen.getByText("Sin eventos todavía.")).toBeInTheDocument();
  });

  it("closes when the labelled X button is clicked", async () => {
    const onClose = vi.fn();
    render(<ProviderPanel onClose={onClose} onChanged={() => {}} />);
    const closeButton = await screen.findByRole("button", { name: "Cerrar" });
    expect(closeButton).toBeInTheDocument();
    await userEvent.click(closeButton);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("shows featured providers first and the rest collapsed", async () => {
    render(<ProviderPanel onClose={() => {}} onChanged={() => {}} />);
    expect(await screen.findByText("ChatGPT")).toBeInTheDocument();
    expect(screen.getByText("Recomendados")).toBeInTheDocument();
    const others = screen.getByRole("button", { name: "Otros proveedores (1)" });
    expect(others).toBeInTheDocument();
    expect(screen.queryByText("Gemini")).not.toBeInTheDocument();
    await userEvent.click(others);
    expect(await screen.findByText("Gemini")).toBeInTheDocument();
  });

  it("connects an API key once, clears the input, and reports the change", async () => {
    const onChanged = vi.fn();
    render(<ProviderPanel onClose={() => {}} onChanged={onChanged} />);
    const connect = await screen.findByRole("button", { name: "Conectar", hidden: false });
    await userEvent.click(connect);
    const input = await screen.findByPlaceholderText("Clave de acceso");
    await userEvent.type(input, "sk-secret");
    await userEvent.click(screen.getByRole("button", { name: "Conectar" }));
    expect(invokeMock).toHaveBeenCalledWith("provider_connect_key", {
      providerId: "openai",
      key: "sk-secret",
      label: null,
    });
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect((input as HTMLInputElement).value).toBe("");
    expect(screen.getByText("Conectado.")).toBeInTheDocument();
  });

  it("shows a human error when the key is invalid", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      const models = modelCommands(cmd);
      if (models) return models;
      if (cmd === "provider_list") return Promise.resolve([openaiSummary]);
      if (cmd === "provider_detail") return Promise.resolve(detail);
      if (cmd === "provider_connect_key") {
        return Promise.reject({ code: "credential_invalid", message: "Esta clave no es válida." });
      }
      return Promise.reject(new Error(`unexpected invoke ${cmd}`));
    });
    render(<ProviderPanel onClose={() => {}} onChanged={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: "Conectar", hidden: false }));
    await userEvent.type(await screen.findByPlaceholderText("Clave de acceso"), "sk-bad");
    await userEvent.click(screen.getByRole("button", { name: "Conectar" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Esta clave no es válida.");
  });

  it("runs a connection test and shows the outcome", async () => {
    render(<ProviderPanel onClose={() => {}} onChanged={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: "Conectar", hidden: false }));
    await userEvent.click(await screen.findByRole("button", { name: "Probar conexión" }));
    await waitFor(() => expect(screen.getByText("Conectado.")).toBeInTheDocument());
  });

  it("allows disconnecting after connecting (refreshed detail)", async () => {
    let connected = false;
    invokeMock.mockImplementation((cmd: string) => {
      const models = modelCommands(cmd);
      if (models) return models;
      if (cmd === "provider_list") return Promise.resolve([openaiSummary]);
      if (cmd === "provider_detail") {
        return Promise.resolve(
          connected ? { ...detail, connections: [{ id: "cred-1", label: null }] } : detail,
        );
      }
      if (cmd === "provider_connect_key") {
        connected = true;
        return Promise.resolve({ id: "cred-1", label: null });
      }
      if (cmd === "provider_disconnect") return Promise.resolve(undefined);
      return Promise.reject(new Error(`unexpected invoke ${cmd}`));
    });
    render(<ProviderPanel onClose={() => {}} onChanged={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: "Conectar", hidden: false }));
    await userEvent.type(await screen.findByPlaceholderText("Clave de acceso"), "sk-x");
    await userEvent.click(screen.getByRole("button", { name: "Conectar" }));
    const disconnect = await screen.findByRole("button", { name: "Desconectar" });
    await userEvent.click(disconnect);
    expect(invokeMock).toHaveBeenCalledWith("provider_disconnect", {
      credentialId: "cred-1",
    });
  });

  it("runs the OAuth flow: begin, open, poll to complete, then disconnect", async () => {
    let completed = false;
    invokeMock.mockImplementation((cmd: string) => {
      const models = modelCommands(cmd);
      if (models) return models;
      if (cmd === "provider_list") return Promise.resolve([openaiWithOauth]);
      if (cmd === "provider_detail") {
        return Promise.resolve(
          completed
            ? { ...detail, connections: [{ id: "cred-9", label: null }] }
            : { ...detail, authMethods: openaiWithOauth.authMethods },
        );
      }
      if (cmd === "provider_oauth_begin") {
        return Promise.resolve({
          attemptId: "att-1",
          url: "https://example.test/oauth/chatgpt-browser",
          instructions: "Abrí el enlace y aprobá el acceso.",
          mode: "auto",
        });
      }
      if (cmd === "provider_oauth_status") {
        completed = true;
        return Promise.resolve({ status: "complete", message: null });
      }
      if (cmd === "provider_oauth_open") return Promise.resolve(undefined);
      if (cmd === "provider_disconnect") return Promise.resolve(undefined);
      return Promise.reject(new Error(`unexpected invoke ${cmd}`));
    });
    render(<ProviderPanel onClose={() => {}} onChanged={() => {}} />);
    await userEvent.click(await screen.findByRole("button", { name: "Conectar" }));
    await userEvent.click(screen.getByRole("button", { name: "Conectá tu cuenta" }));
    expect(screen.getByText("https://example.test/oauth/chatgpt-browser")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Abrir en el navegador" }));
    expect(invokeMock).toHaveBeenCalledWith("provider_oauth_open", {
      url: "https://example.test/oauth/chatgpt-browser",
    });
    // The poll runs every 2s and resolves to `complete` on the first tick; the
    // card then refreshes its detail so Desconectar is available.
    const disconnect = await screen.findByRole(
      "button",
      { name: "Desconectar" },
      {
        timeout: 5000,
      },
    );
    await userEvent.click(disconnect);
    expect(invokeMock).toHaveBeenCalledWith("provider_disconnect", {
      credentialId: "cred-9",
    });
  });
});
