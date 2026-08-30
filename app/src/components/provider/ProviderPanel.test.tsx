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

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockImplementation((cmd: string) => {
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
    return Promise.reject(new Error(`unexpected invoke ${cmd}`));
  });
});

describe("ProviderPanel", () => {
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

  it("connects an API key once and reports the change", async () => {
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
  });

  it("shows a human error when the key is invalid", async () => {
    invokeMock.mockImplementation((cmd: string) => {
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
});
