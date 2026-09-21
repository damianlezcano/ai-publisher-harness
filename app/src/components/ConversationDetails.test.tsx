import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import ConversationDetails from "./ConversationDetails";
import type { ProjectView } from "../types";
import { humanDate, humanSize } from "../messages";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const invokeMock = vi.mocked(invoke);

const project = (id: string): ProjectView => ({
  id,
  name: `Conversación ${id}`,
  materials: [],
  creations: [],
  messages: [],
  publication: { state: "local", publicUrl: null },
  model: null,
});

const providerUsage = {
  conversationId: "a",
  turnId: "turn-a",
  provider: "Proveedor con nombre muy largo para comprobar el ajuste seguro",
  model: "modelo-grande-para-pruebas",
  inputTokens: 12450,
  outputTokens: 820,
  cacheReadTokens: 32,
  cacheWriteTokens: null,
  totalTokens: null,
  costUsd: 0.014,
  turnDurationMs: 3480,
  source: "provider_actual",
};

const knowledge = {
  conversationId: "a",
  materialCount: 3,
  corpusBytes: 252600,
  corpusUtf8Chars: 248100,
  corpusEstTokens: 84200,
  retrievalCandidateCount: 17,
  selectedEvidenceCount: 4,
  selectedEvidenceBytes: 6540,
  selectedEvidenceUtf8Chars: 6402,
  evidenceEstTokens: 2180,
  contextReductionPct: 97.4,
  semanticProviderState: "available",
  requestPreparationMs: 84,
  retrievalMode: "normal",
  exhaustiveCoverage: "not_requested",
};

const detailProject: ProjectView = {
  ...project("0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22"),
  name: "Fotosíntesis",
  materials: [
    {
      id: "m1",
      displayName: "manual.pdf",
      originalFileName: "manual.pdf",
      kind: "pdf",
      byteSize: 10,
      createdAt: "2026-08-28T15:00:00Z",
    },
  ],
  creations: [
    {
      id: "c1",
      displayName: "Actividad",
      kind: "web",
      visibility: "private",
      byteSize: 20,
      createdAt: "2026-08-28T15:00:00Z",
      revision: 1,
      lineageId: "c1",
      versionNumber: 1,
      isCurrent: true,
      availableVersionIds: ["c1"],
    },
  ],
};
const model = {
  providerId: "opencode",
  modelId: "big-pickle",
  name: "Big Pickle",
  free: true,
  recommended: true,
  deprecated: false,
};

beforeEach(() => {
  invokeMock.mockReset();
  invokeMock.mockImplementation((command: string) => {
    if (command === "model_list" || command === "provider_list") return Promise.resolve([]);
    if (command === "session_logs") return Promise.resolve([]);
    return Promise.resolve(undefined);
  });
});

describe("ConversationDetails metrics", () => {
  it("renders explicit durable metrics ahead of conflicting session-log fallback", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list" || command === "provider_list") return Promise.resolve([]);
      if (command === "conversation_last_turn_metrics")
        return Promise.resolve({
          provider: "durable-provider",
          model: "durable-model",
          inputTokens: 4321,
          outputTokens: 876,
          cacheReadTokens: null,
          cacheWriteTokens: null,
          totalTokens: null,
          costUsd: 0.123,
          turnDurationMs: 88,
          source: "provider_actual",
          remoteCalls: 2,
          materialCount: 4,
          corpusBytes: 9876,
          corpusUtf8Chars: 9000,
          corpusEstTokens: 2222,
          retrievalCandidateCount: 9,
          selectedEvidenceCount: 3,
          selectedEvidenceBytes: 444,
          selectedEvidenceUtf8Chars: 400,
          evidenceEstTokens: 111,
          contextReductionPct: 95,
          semanticProviderState: "available",
          requestPreparationMs: 12,
          retrievalMode: "normal",
          exhaustiveCoverage: "not_requested",
        });
      if (command === "session_logs")
        return Promise.resolve([
          { level: "INFO", message: "fallback", usage: { ...providerUsage, inputTokens: 999999 } },
        ]);
      return Promise.resolve(undefined);
    });
    render(
      <ConversationDetails
        project={project("a")}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    expect(await screen.findByText("4.321 tokens")).toBeVisible();
    expect(screen.getByText("USD 0.123")).toBeVisible();
    expect(screen.getByText("2.222 tokens estimados")).toBeVisible();
    expect(screen.queryByText("999.999 tokens")).not.toBeInTheDocument();
  });

  it("shows corpus-wide thematic synthesis metrics as a distinct retrieval mode", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list" || command === "provider_list") return Promise.resolve([]);
      if (command === "conversation_last_turn_metrics")
        return Promise.resolve({
          materialCount: 15,
          corpusBytes: 500000,
          corpusUtf8Chars: 480000,
          corpusEstTokens: 166666,
          retrievalCandidateCount: 54,
          selectedEvidenceCount: 14,
          evidenceEstTokens: 2172,
          contextReductionPct: 91,
          semanticProviderState: "not_requested",
          requestPreparationMs: 9,
          retrievalMode: "thematic",
          exhaustiveCoverage: "not_requested",
          eligibleMaterials: 15,
          materialsInspected: 14,
          chunksInspected: 57,
        });
      if (command === "session_logs") return Promise.resolve([]);
      return Promise.resolve(undefined);
    });
    render(
      <ConversationDetails
        project={project("a")}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    expect(await screen.findByText("thematic")).toBeVisible();
    expect(screen.getByText("Materiales que aportan temas")).toBeVisible();
    expect(screen.getAllByText("14").length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("Candidatos de temas recurrentes")).toBeVisible();
    expect(screen.getAllByText("54").length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("2.172")).toBeVisible();
  });

  it("preserves name, model, resource rows, folder actions, and close behavior", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list") return Promise.resolve([model]);
      if (command === "provider_list" || command === "session_logs") return Promise.resolve([]);
      return Promise.resolve(undefined);
    });
    const onRefresh = vi.fn();
    const onClose = vi.fn();
    const user = userEvent.setup();
    render(
      <ConversationDetails
        project={detailProject}
        active={false}
        onClose={onClose}
        onRefresh={onRefresh}
      />,
    );
    expect(screen.getByLabelText("Nombre")).toHaveValue("Fotosíntesis");
    expect(await screen.findByRole("option", { name: /Big Pickle/ })).toBeInTheDocument();
    const materialRow = screen.getByText("manual.pdf").closest("li");
    expect(materialRow).toHaveClass("item-row");
    expect(materialRow?.textContent).toContain(humanSize(10));
    expect(materialRow?.textContent).toContain(humanDate(detailProject.materials[0].createdAt));
    fireEvent.change(screen.getByLabelText("Nombre"), { target: { value: "Nueva" } });
    await user.click(screen.getByRole("button", { name: "Renombrar" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("project_rename", {
        projectId: detailProject.id,
        name: "Nueva",
      }),
    );
    await user.selectOptions(
      screen.getByLabelText("Modelo de esta conversación"),
      "opencode::big-pickle",
    );
    expect(invokeMock).toHaveBeenCalledWith("conversation_model_select", {
      projectId: detailProject.id,
      providerId: "opencode",
      modelId: "big-pickle",
    });
    const folders = screen.getAllByRole("button", { name: "Abrir carpeta contenedora" });
    await user.click(folders[0]);
    await user.click(folders[1]);
    expect(invokeMock).toHaveBeenCalledWith("materials_open_folder", {
      projectId: detailProject.id,
    });
    expect(invokeMock).toHaveBeenCalledWith("creations_open_folder", {
      projectId: detailProject.id,
    });
    await user.click(screen.getByRole("button", { name: "Cerrar" }));
    expect(onClose).toHaveBeenCalledOnce();
  });

  it("preserves safe text and image previews and active-model locking", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list") return Promise.resolve([model]);
      if (command === "provider_list" || command === "session_logs") return Promise.resolve([]);
      if (command === "preview_data")
        return Promise.resolve({ contentType: "text/markdown", dataBase64: btoa("<b>Hola</b>") });
      return Promise.resolve(undefined);
    });
    const user = userEvent.setup();
    const view = render(
      <ConversationDetails
        project={detailProject}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    await user.click(screen.getByRole("button", { name: "Abrir: manual.pdf" }));
    expect(
      (await screen.findByRole("dialog", { name: "manual.pdf" })).querySelector("pre"),
    ).toBeInTheDocument();
    view.unmount();
    render(
      <ConversationDetails
        project={{ ...detailProject, model: { providerId: "opencode", modelId: "big-pickle" } }}
        active
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    expect(await screen.findByLabelText("Modelo de esta conversación")).toBeDisabled();
  });
  it("shows selected-conversation provider telemetry apart from Knowledge estimates", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list" || command === "provider_list") return Promise.resolve([]);
      if (command === "session_logs") {
        return Promise.resolve([
          { level: "INFO", message: "structural only", knowledge },
          { level: "INFO", message: "structural only", usage: providerUsage },
          {
            level: "INFO",
            message: "secret prompt text must not render",
            usage: { ...providerUsage, conversationId: "b", inputTokens: 999999 },
          },
        ]);
      }
      return Promise.resolve(undefined);
    });
    render(
      <ConversationDetails
        project={project("a")}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );

    await screen.findByRole("heading", { name: "Uso y optimización" });
    expect(screen.getByRole("heading", { name: "Último turno" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Uso real del proveedor" })).toBeVisible();
    expect(screen.getByRole("heading", { name: /Optimización Knowledge/ })).toBeVisible();
    expect(screen.getByText("12.450 tokens")).toBeVisible();
    expect(screen.getByText("USD 0.014")).toBeVisible();
    expect(screen.getByText("84.200 tokens estimados")).toBeVisible();
    expect(screen.getByText("2.180 tokens estimados")).toBeVisible();
    expect(screen.getByText("97,4 %")).toBeVisible();
    expect(screen.queryByText("999.999 tokens")).not.toBeInTheDocument();
    expect(screen.queryByText("secret prompt text must not render")).not.toBeInTheDocument();
    expect(screen.getByRole("dialog", { name: "Detalles de la conversación" })).toHaveClass(
      "conversation-details-dialog",
    );
  });

  it("renders missing provider fields as No disponible rather than zero", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list" || command === "provider_list") return Promise.resolve([]);
      if (command === "session_logs")
        return Promise.resolve([
          {
            level: "INFO",
            message: "structural",
            usage: {
              ...providerUsage,
              inputTokens: 31,
              outputTokens: null,
              cacheReadTokens: null,
              cacheWriteTokens: null,
              costUsd: null,
              turnDurationMs: null,
              source: "estimated",
            },
          },
        ]);
      return Promise.resolve(undefined);
    });
    render(
      <ConversationDetails
        project={project("a")}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    await waitFor(() =>
      expect(screen.getAllByText("No disponible").length).toBeGreaterThanOrEqual(6),
    );
    expect(screen.queryByText("0 tokens")).not.toBeInTheDocument();
    expect(screen.queryByText("31 tokens")).not.toBeInTheDocument();
  });

  it("uses the selected conversation only when switching detail instances", async () => {
    invokeMock.mockImplementation((command: string) => {
      if (command === "model_list" || command === "provider_list") return Promise.resolve([]);
      if (command === "session_logs")
        return Promise.resolve([
          { level: "INFO", message: "structural", usage: providerUsage },
          {
            level: "INFO",
            message: "structural",
            usage: {
              ...providerUsage,
              conversationId: "b",
              inputTokens: 77,
              provider: "otro-proveedor",
            },
          },
        ]);
      return Promise.resolve(undefined);
    });
    const view = render(
      <ConversationDetails
        project={project("a")}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    expect(await screen.findByText("12.450 tokens")).toBeVisible();
    view.rerender(
      <ConversationDetails
        project={project("b")}
        active={false}
        onClose={() => {}}
        onRefresh={() => {}}
      />,
    );
    expect(await screen.findByText("77 tokens")).toBeVisible();
    expect(screen.queryByText("12.450 tokens")).not.toBeInTheDocument();
  });
});
