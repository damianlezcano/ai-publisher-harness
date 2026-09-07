import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import ComposerBar from "./ComposerBar";
import { messages } from "../messages";
import type { MaterialView, ModelSummary, ProviderSummary, SelectedModelView } from "../types";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const invokeMock = vi.mocked(invoke);
const openDialogMock = vi.mocked(openDialog);

const projectId = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22";

const materials: MaterialView[] = [
  {
    id: "m1",
    displayName: "diagrama.png",
    originalFileName: "diagrama.png",
    kind: "image",
    byteSize: 1024,
    createdAt: "2026-08-28T15:00:00Z",
  },
  {
    id: "m2",
    displayName: "foto.png",
    originalFileName: "foto.png",
    kind: "image",
    byteSize: 3,
    createdAt: "2026-08-28T15:00:00Z",
  },
];

const freeModel: ModelSummary = {
  providerId: "opencode",
  modelId: "big-pickle",
  name: "Big Pickle",
  free: true,
  recommended: true,
  deprecated: false,
};

const paidModel: ModelSummary = {
  providerId: "openai",
  modelId: "gpt-4",
  name: "GPT-4",
  free: false,
  recommended: false,
  deprecated: false,
};

const selectedFreeModel: SelectedModelView = {
  model: freeModel,
  notice: null,
  requiresChoice: false,
};

const requiresChoiceModel: SelectedModelView = {
  model: paidModel,
  notice: "Elegí un modelo de pago conectado.",
  requiresChoice: true,
};

const base = {
  projectId,
  materials,
  agentPhase: "idle" as const,
  aiUsable: true,
  onSend: vi.fn(),
  onCancel: vi.fn(),
  onMaterialsChanged: vi.fn(),
};

function pasteImage(textarea: HTMLTextAreaElement) {
  const file = new File([new Uint8Array([1, 2, 3])], "foto.png", { type: "image/png" });
  fireEvent.paste(textarea, {
    clipboardData: {
      items: [
        {
          kind: "file",
          type: "image/png",
          getAsFile: () => file,
        },
      ],
    },
  });
}

function setupApiMock(responses: {
  models?: ModelSummary[];
  providers?: ProviderSummary[];
  selected?: SelectedModelView;
  addImage?: unknown;
  selectResult?: void;
  stageError?: unknown;
}) {
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "model_list") return Promise.resolve(responses.models ?? []);
    if (cmd === "provider_list") return Promise.resolve(responses.providers ?? []);
    if (cmd === "model_get_selected") return Promise.resolve(responses.selected ?? null);
    if (cmd === "material_add_image")
      return Promise.resolve(responses.addImage ?? { material: materials[0], duplicate: false });
    if (cmd === "model_select") return Promise.resolve(responses.selectResult ?? undefined);
    if (cmd === "attachments_stage_paths") {
      if (responses.stageError) return Promise.reject(responses.stageError);
      return Promise.resolve({ items: [{ sourceName: "diagrama.png", status: "ready" }] });
    }
    return Promise.reject(new Error(`unexpected invoke: ${cmd}`));
  });
}

beforeEach(() => {
  invokeMock.mockReset();
  openDialogMock.mockReset();
  base.onSend.mockReset();
  base.onCancel.mockReset();
  base.onMaterialsChanged.mockReset();
});

describe("ComposerBar", () => {
  it("renders the prompt textarea and Enviar button; typing then clicking Enviar calls onSend with the trimmed prompt", async () => {
    setupApiMock({ selected: selectedFreeModel });
    render(<ComposerBar {...base} />);
    const textarea = screen.getByLabelText("Pedido a la IA") as HTMLTextAreaElement;
    await userEvent.type(textarea, "  Creá una actividad  ");
    await userEvent.click(screen.getByRole("button", { name: "Enviar" }));
    expect(base.onSend).toHaveBeenCalledWith("Creá una actividad", []);
    expect(textarea).toHaveValue("");
  });

  it("Enter sends; Shift+Enter inserts a newline without sending", async () => {
    setupApiMock({ selected: selectedFreeModel });
    render(<ComposerBar {...base} />);
    const textarea = screen.getByLabelText("Pedido a la IA") as HTMLTextAreaElement;
    await userEvent.type(textarea, "Primera línea");
    fireEvent.keyDown(textarea, { key: "Enter", code: "Enter", shiftKey: true });
    expect(base.onSend).not.toHaveBeenCalled();
    fireEvent.keyDown(textarea, { key: "Enter", code: "Enter" });
    await waitFor(() => expect(base.onSend).toHaveBeenCalledWith("Primera línea", []));
  });

  it("does not send while IME composition is active", async () => {
    setupApiMock({ selected: selectedFreeModel });
    render(<ComposerBar {...base} />);
    const textarea = screen.getByLabelText("Pedido a la IA") as HTMLTextAreaElement;
    await userEvent.type(textarea, "hola");
    fireEvent.keyDown(textarea, { key: "Enter", code: "Enter", isComposing: true });
    expect(base.onSend).not.toHaveBeenCalled();
    fireEvent.keyDown(textarea, { key: "Enter", code: "Enter", keyCode: 229 });
    expect(base.onSend).not.toHaveBeenCalled();
  });

  it("does not send whitespace-only prompts", async () => {
    setupApiMock({ selected: selectedFreeModel });
    render(<ComposerBar {...base} />);
    const textarea = screen.getByLabelText("Pedido a la IA") as HTMLTextAreaElement;
    await userEvent.type(textarea, "   ");
    fireEvent.keyDown(textarea, { key: "Enter", code: "Enter" });
    expect(base.onSend).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "Enviar" })).toBeDisabled();
  });

  it("does not send when the composer is busy", async () => {
    setupApiMock({ selected: selectedFreeModel });
    render(<ComposerBar {...base} agentPhase="working" />);
    const textarea = screen.getByLabelText("Pedido a la IA");
    fireEvent.keyDown(textarea, { key: "Enter", code: "Enter" });
    expect(base.onSend).not.toHaveBeenCalled();
  });

  it("does not send an empty prompt", async () => {
    setupApiMock({ selected: selectedFreeModel });
    render(<ComposerBar {...base} />);
    await userEvent.click(screen.getByRole("button", { name: "Enviar" }));
    expect(base.onSend).not.toHaveBeenCalled();
  });

  it("working state shows Cancelar (calls onCancel) and disables the textarea", async () => {
    setupApiMock({ selected: selectedFreeModel });
    render(<ComposerBar {...base} agentPhase="working" />);
    expect(screen.getByRole("button", { name: "Cancelar" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Enviar" })).not.toBeInTheDocument();
    expect(screen.getByLabelText("Pedido a la IA")).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "Cancelar" }));
    expect(base.onCancel).toHaveBeenCalledOnce();
  });

  it("requiresChoice (aiUsable=false) disables the composer", async () => {
    setupApiMock({ selected: requiresChoiceModel });
    render(<ComposerBar {...base} aiUsable={false} />);
    expect(screen.getByLabelText("Pedido a la IA")).toBeDisabled();
    expect(screen.getByRole("button", { name: "Enviar" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Adjuntar" })).toBeDisabled();
  });

  it("Adjuntar stages locally and becomes a removable draft chip without material import", async () => {
    setupApiMock({ selected: selectedFreeModel });
    openDialogMock.mockResolvedValueOnce("/tmp/diagrama.png");
    render(<ComposerBar {...base} />);
    expect(screen.queryByRole("button", { name: "diagrama.png" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Adjuntar" }));
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("attachments_stage_paths", {
        paths: ["/tmp/diagrama.png"],
      }),
    );
    expect(invokeMock.mock.calls.some((call) => call[0] === "material_add_from_path")).toBe(false);
    expect(screen.getByRole("button", { name: "Quitar diagrama.png" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Quitar diagrama.png" }));
    expect(screen.queryByRole("button", { name: "Quitar diagrama.png" })).not.toBeInTheDocument();
  });

  it("does not render a persistent material picker that could suggest earlier files", () => {
    setupApiMock({ selected: selectedFreeModel });
    render(<ComposerBar {...base} />);
    // The attach button must not expand a list of previously uploaded files.
    expect(screen.queryByText(messages.material.addFile)).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "diagrama.png" })).not.toBeInTheDocument();
  });

  it("clears controlled attachment chips after send while materials remain available", async () => {
    setupApiMock({ selected: selectedFreeModel });
    const onSend = vi.fn();
    const onAttachmentIdsChange = vi.fn();
    render(
      <ComposerBar
        {...base}
        onSend={onSend}
        attachmentIds={["m1"]}
        onAttachmentIdsChange={onAttachmentIdsChange}
      />,
    );
    expect(screen.getByText("diagrama.png")).toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("Pedido a la IA"), "hola");
    await userEvent.click(screen.getByRole("button", { name: "Enviar" }));
    expect(onAttachmentIdsChange).toHaveBeenCalledWith([]);
    expect(screen.getByText("diagrama.png")).toBeInTheDocument();
    expect(onSend).toHaveBeenCalledWith("hola", ["m1"]);
  });

  it("stages a pasted image locally without durable material or Knowledge work, and removes it cleanly", async () => {
    setupApiMock({ selected: selectedFreeModel });
    render(<ComposerBar {...base} />);
    const textarea = screen.getByLabelText("Pedido a la IA") as HTMLTextAreaElement;
    pasteImage(textarea);
    await waitFor(() => expect(screen.getByText("foto.png")).toBeInTheDocument());
    expect(invokeMock.mock.calls.some((call) => call[0] === "material_add_image")).toBe(false);
    expect(invokeMock.mock.calls.some((call) => call[0] === "agent_send_staged")).toBe(false);
    expect(base.onMaterialsChanged).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole("button", { name: "Quitar foto.png" }));
    expect(screen.queryByText("foto.png")).not.toBeInTheDocument();
    await userEvent.type(textarea, "Creá una actividad");
    await userEvent.click(screen.getByRole("button", { name: "Enviar" }));
    expect(base.onSend).toHaveBeenCalledWith("Creá una actividad", []);
  });

  it("passes pasted-image bytes to the accepted send boundary only after Enviar", async () => {
    setupApiMock({ selected: selectedFreeModel });
    render(<ComposerBar {...base} />);
    const textarea = screen.getByLabelText("Pedido a la IA") as HTMLTextAreaElement;
    pasteImage(textarea);
    await waitFor(() => expect(screen.getByText("foto.png")).toBeInTheDocument());
    await userEvent.type(textarea, "Usá la captura");
    await userEvent.click(screen.getByRole("button", { name: "Enviar" }));
    await waitFor(() =>
      expect(base.onSend).toHaveBeenCalledWith(
        "Usá la captura",
        [expect.stringMatching(/^clipboard-/)],
        [
          expect.objectContaining({
            stagingId: expect.stringMatching(/^clipboard-/),
            fileName: "foto.png",
            contentType: "image/png",
          }),
        ],
      ),
    );
    expect(invokeMock.mock.calls.some((call) => call[0] === "material_add_image")).toBe(false);
  });

  it("does not render a model selector; attachment, prompt and send only", async () => {
    setupApiMock({ selected: selectedFreeModel });
    render(<ComposerBar {...base} />);
    expect(screen.getByLabelText("Pedido a la IA")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Enviar" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Adjuntar" })).toBeInTheDocument();
    expect(screen.queryByLabelText("Modelo")).not.toBeInTheDocument();
    expect(screen.queryByText("Big Pickle / Gratis")).not.toBeInTheDocument();
  });

  it("shareAction node renders when provided", () => {
    setupApiMock({ selected: selectedFreeModel });
    render(<ComposerBar {...base} shareAction={<button type="button">Compartir</button>} />);
    expect(screen.getByRole("button", { name: "Compartir" })).toBeInTheDocument();
  });

  it("renders controlled attachment ids as chips and notifies parent on remove", async () => {
    const onAttachmentIdsChange = vi.fn();
    setupApiMock({ selected: selectedFreeModel });
    render(
      <ComposerBar
        {...base}
        attachmentIds={["m1"]}
        onAttachmentIdsChange={onAttachmentIdsChange}
      />,
    );
    expect(screen.getByRole("button", { name: "Quitar diagrama.png" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Quitar diagrama.png" }));
    expect(onAttachmentIdsChange).toHaveBeenCalledWith([]);
  });

  it("sends controlled attachment ids and clears them through onAttachmentIdsChange", async () => {
    const onAttachmentIdsChange = vi.fn();
    setupApiMock({ selected: selectedFreeModel });
    render(
      <ComposerBar
        {...base}
        attachmentIds={["m2"]}
        onAttachmentIdsChange={onAttachmentIdsChange}
      />,
    );
    const textarea = screen.getByLabelText("Pedido a la IA") as HTMLTextAreaElement;
    await userEvent.type(textarea, "Usá estos datos");
    await userEvent.click(screen.getByRole("button", { name: "Enviar" }));
    expect(base.onSend).toHaveBeenCalledWith("Usá estos datos", ["m2"]);
    expect(onAttachmentIdsChange).toHaveBeenCalledWith([]);
  });

  it("keeps pending attachments when the turn is rejected before acceptance", async () => {
    const onAttachmentIdsChange = vi.fn();
    const rejected = vi.fn().mockRejectedValue({ code: "ai_unavailable" });
    setupApiMock({ selected: selectedFreeModel });
    render(
      <ComposerBar
        {...base}
        onSend={rejected}
        attachmentIds={["m1"]}
        onAttachmentIdsChange={onAttachmentIdsChange}
      />,
    );
    await userEvent.type(screen.getByLabelText("Pedido a la IA"), "Reintentá");
    await userEvent.click(screen.getByRole("button", { name: "Enviar" }));
    await waitFor(() => expect(rejected).toHaveBeenCalledWith("Reintentá", ["m1"]));
    expect(onAttachmentIdsChange).not.toHaveBeenCalledWith([]);
    expect(screen.getByRole("button", { name: "Quitar diagrama.png" })).toBeInTheDocument();
  });

  it("shows a plain-language error when adding a picked file is rejected", async () => {
    openDialogMock.mockResolvedValueOnce("/tmp/bad.exe");
    setupApiMock({
      selected: selectedFreeModel,
      stageError: {
        code: "material_unsupported",
        message: "No admitimos ese tipo de archivo.",
      },
    });
    render(<ComposerBar {...base} materials={[]} />);
    await userEvent.click(screen.getByRole("button", { name: "Adjuntar" }));
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent("No admitimos ese tipo de archivo."),
    );
    expect(screen.queryByText("material_unsupported")).not.toBeInTheDocument();
    expect(screen.queryByText("/tmp/bad.exe")).not.toBeInTheDocument();
    expect(base.onMaterialsChanged).not.toHaveBeenCalled();
  });

  it("collapses a large attachment set to a count toggle instead of unbounded chips", async () => {
    setupApiMock({ selected: selectedFreeModel });
    const manyIds = Array.from({ length: 51 }, (_, i) => `m${i}`);
    render(
      <ComposerBar
        {...base}
        materials={manyIds.map((id, i) => ({
          id,
          displayName: `nota-${i}.md`,
          originalFileName: `nota-${i}.md`,
          kind: "text",
          byteSize: 128,
          createdAt: "2026-08-28T15:00:00Z",
        }))}
        attachmentIds={manyIds}
        onAttachmentIdsChange={() => {}}
      />,
    );
    // Collapsed: the send button stays reachable and a 51-file selection never
    // becomes an inline wall of pending chips.
    expect(screen.getByRole("button", { name: "Enviar" })).toBeInTheDocument();
    expect(screen.getByText(/51 archivos seleccionados · Ver todos/)).toBeInTheDocument();
    expect(screen.queryByText("nota-0.md")).not.toBeInTheDocument();

    await userEvent.click(
      screen.getByRole("button", { name: /51 archivos seleccionados · Ver todos/ }),
    );
    expect(screen.getByText("nota-50.md")).toBeInTheDocument();
    // Expanding flips the toggle to "Ver menos".
    expect(screen.getByRole("button", { name: "Ver menos" })).toBeInTheDocument();
  });

  it.each([1, 5, 6, 16, 51, 100])("keeps a %i-file pending selection bounded", (count) => {
    setupApiMock({ selected: selectedFreeModel });
    const ids = Array.from({ length: count }, (_, i) => `m${i}`);
    render(
      <ComposerBar
        {...base}
        materials={ids.map((id) => ({
          id,
          displayName: `${id}.txt`,
          originalFileName: `${id}.txt`,
          kind: "text",
          byteSize: 1,
          createdAt: "2026-08-28T15:00:00Z",
        }))}
        attachmentIds={ids}
        onAttachmentIdsChange={() => {}}
      />,
    );
    expect(screen.getByRole("button", { name: "Enviar" })).toBeInTheDocument();
    if (count <= 5) {
      expect(screen.getByText("m0.txt")).toBeInTheDocument();
    } else {
      expect(screen.getByText(`${count} archivos seleccionados · Ver todos`)).toBeInTheDocument();
      expect(screen.queryByText("m0.txt")).not.toBeInTheDocument();
    }
  });
});
