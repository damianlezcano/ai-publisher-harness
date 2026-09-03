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
  it("renders current-session logs and clears them", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "provider_list") return Promise.resolve([]);
      if (cmd === "session_logs")
        return Promise.resolve([{ level: "INFO", message: "turn started" }]);
      if (cmd === "session_logs_clear") return Promise.resolve(undefined);
      return Promise.resolve(undefined);
    });
    render(<ProviderPanel onClose={() => {}} onChanged={() => {}} />);
    const dialog = await screen.findByRole("dialog", { name: "Configuración" });
    expect(dialog).toHaveClass("provider-dialog");
    expect(dialog.querySelector(".dialog-body")).not.toBeNull();
    const logs = await screen.findByText("[INFO] turn started");
    expect(logs.closest("pre")).toHaveClass("session-logs");
    await userEvent.click(screen.getByRole("button", { name: "Limpiar" }));
    expect(invokeMock).toHaveBeenCalledWith("session_logs_clear");
    expect(screen.getByText("Sin eventos todavía.")).toBeInTheDocument();
  });

  it("renders as a labelled dialog and closes on Escape", async () => {
    const onClose = vi.fn();
    render(<ProviderPanel onClose={onClose} onChanged={() => {}} />);
    expect(await screen.findByRole("dialog", { name: "Configuración" })).toBeInTheDocument();
    expect(await screen.findByText("Logs de esta sesión")).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("keeps the Configuración -> Logs de esta sesión contract visible and bounded", async () => {
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

    // The heading is the first section of the modal body: reachable without
    // scrolling, never hidden by the provider list.
    const heading = screen.getByRole("heading", { name: "Logs de esta sesión" });
    const section = heading.closest("section");
    expect(section).toHaveAttribute("aria-label", "Logs de esta sesión");
    expect(
      dialog.querySelector(".dialog-body > section[aria-label='Logs de esta sesión']"),
    ).not.toBeNull();

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
