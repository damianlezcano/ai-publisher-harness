import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import ModelSelector from "./ModelSelector";
import type { ModelSummary, ProviderSummary } from "../../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

const models: ModelSummary[] = [
  {
    providerId: "opencode",
    modelId: "big-pickle",
    name: "big-pickle",
    free: true,
    recommended: true,
    deprecated: false,
  },
  {
    providerId: "opencode",
    modelId: "mimo-free",
    name: "mimo",
    free: true,
    recommended: false,
    deprecated: false,
  },
  {
    providerId: "openai",
    modelId: "gpt-4o",
    name: "gpt-4o",
    free: false,
    recommended: true,
    deprecated: false,
  },
];

const providers: ProviderSummary[] = [
  {
    id: "opencode",
    name: "Gratis",
    authMethods: [],
    connected: false,
    connectionLabel: null,
    highlighted: true,
  },
  {
    id: "openai",
    name: "ChatGPT",
    authMethods: [],
    connected: true,
    connectionLabel: "clave",
    highlighted: true,
  },
];

function selected(model: ModelSummary, requiresChoice = false, notice: string | null = null) {
  return { model, notice, requiresChoice };
}

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "model_list") return Promise.resolve(models);
    if (cmd === "provider_list") return Promise.resolve(providers);
    if (cmd === "model_get_selected") {
      return Promise.resolve(selected(models[0]));
    }
    if (cmd === "model_select") return Promise.resolve(undefined);
    return Promise.reject(new Error(`unexpected invoke ${cmd}`));
  });
});

describe("ModelSelector", () => {
  it("shows the default free recommended model with a Gratis badge", async () => {
    render(<ModelSelector refreshKey={0} />);
    await waitFor(() => expect(screen.getByLabelText("Modelo")).toBeInTheDocument());
    const select = screen.getByLabelText("Modelo") as HTMLSelectElement;
    expect(select.value).toBe("opencode::big-pickle");
    expect(screen.getByText("Gratis")).toBeInTheDocument();
  });

  it("selecting a paid model persists it and shows De pago", async () => {
    let current = models[0];
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "model_list") return Promise.resolve(models);
      if (cmd === "provider_list") return Promise.resolve(providers);
      if (cmd === "model_get_selected") return Promise.resolve(selected(current));
      if (cmd === "model_select") {
        current = models[2];
        return Promise.resolve(undefined);
      }
      return Promise.reject(new Error(`unexpected invoke ${cmd}`));
    });
    render(<ModelSelector refreshKey={0} />);
    const select = await screen.findByLabelText("Modelo");
    await userEvent.selectOptions(select, "openai::gpt-4o");
    expect(invokeMock).toHaveBeenCalledWith("model_select", {
      providerId: "openai",
      modelId: "gpt-4o",
    });
    await waitFor(() =>
      expect(screen.getByText("De pago", { selector: ".model-badge" })).toBeInTheDocument(),
    );
  });

  it("requires an explicit choice when the stored model disappeared", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "model_list") return Promise.resolve(models);
      if (cmd === "provider_list") return Promise.resolve(providers);
      if (cmd === "model_get_selected") {
        return Promise.resolve(
          selected(
            {
              providerId: "openai",
              modelId: "ghost",
              name: "ghost",
              free: false,
              recommended: false,
              deprecated: false,
            },
            true,
            "Este modelo ya no está disponible. Elegí otro.",
          ),
        );
      }
      return Promise.reject(new Error(`unexpected invoke ${cmd}`));
    });
    render(<ModelSelector refreshKey={0} />);
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Este modelo ya no está disponible. Elegí otro.",
    );
  });

  it("surfaces the fallback notice when a stored model was replaced", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "model_list") return Promise.resolve(models);
      if (cmd === "provider_list") return Promise.resolve(providers);
      if (cmd === "model_get_selected") {
        return Promise.resolve(
          selected(models[0], false, "Este modelo ya no está disponible; usamos el recomendado."),
        );
      }
      return Promise.reject(new Error(`unexpected invoke ${cmd}`));
    });
    render(<ModelSelector refreshKey={0} />);
    expect(
      await screen.findByText("Este modelo ya no está disponible; usamos el recomendado."),
    ).toBeInTheDocument();
  });

  it("shows only free and connected-provider models", async () => {
    render(<ModelSelector refreshKey={0} />);
    await waitFor(() => expect(screen.getByLabelText("Modelo")).toBeInTheDocument());
    const select = screen.getByLabelText("Modelo") as HTMLSelectElement;
    const options = Array.from(select.options).map((o) => o.value);
    // big-pickle, mimo-free (free) + gpt-4o (connected openai) are visible;
    // nothing else is offered.
    expect(options).toContain("opencode::big-pickle");
    expect(options).toContain("opencode::mimo-free");
    expect(options).toContain("openai::gpt-4o");
    expect(options.length).toBe(3);
  });
});
