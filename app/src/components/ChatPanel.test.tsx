import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import ChatPanel from "./ChatPanel";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

const projectId = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22";

const materials = [
  {
    id: "m1",
    displayName: "diagrama.png",
    originalFileName: "diagrama.png",
    kind: "image",
    byteSize: 1024,
    createdAt: "2026-08-28T15:00:00Z",
  },
];

const base = {
  projectId,
  materials,
  agentPhase: "idle" as const,
  agentMessage: null as string | null,
  onRefresh: vi.fn(),
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

beforeEach(() => {
  invokeMock.mockReset();
  base.onRefresh.mockReset();
});

describe("ChatPanel", () => {
  it("sends a prompt with attachment ids and clears the input", async () => {
    invokeMock
      .mockResolvedValueOnce({ material: materials[0], duplicate: false })
      .mockResolvedValueOnce(undefined);
    render(<ChatPanel {...base} />);
    const textarea = screen.getByLabelText("Pedido a la IA") as HTMLTextAreaElement;
    pasteImage(textarea);
    await waitFor(() => expect(screen.getByText("diagrama.png")).toBeInTheDocument());
    await userEvent.type(textarea, "Creá una actividad");
    await userEvent.click(screen.getByRole("button", { name: "Enviar" }));
    expect(invokeMock).toHaveBeenLastCalledWith("agent_send", {
      projectId,
      prompt: "Creá una actividad",
      attachmentIds: ["m1"],
    });
    expect(screen.getByLabelText("Pedido a la IA")).toHaveValue("");
    expect(screen.queryByText("diagrama.png")).not.toBeInTheDocument();
  });

  it("renders attachment chips, removes one, and clears on send", async () => {
    invokeMock
      .mockResolvedValueOnce({ material: materials[0], duplicate: false })
      .mockResolvedValueOnce(undefined);
    render(<ChatPanel {...base} />);
    const textarea = screen.getByLabelText("Pedido a la IA") as HTMLTextAreaElement;
    pasteImage(textarea);
    await waitFor(() => expect(screen.getByText("diagrama.png")).toBeInTheDocument());
    await userEvent.click(screen.getByRole("button", { name: "Quitar diagrama.png" }));
    expect(screen.queryByText("diagrama.png")).not.toBeInTheDocument();
    await userEvent.type(textarea, "Hola");
    await userEvent.click(screen.getByRole("button", { name: "Enviar" }));
    expect(invokeMock).toHaveBeenLastCalledWith("agent_send", {
      projectId,
      prompt: "Hola",
      attachmentIds: [],
    });
  });

  it("sends on Ctrl+Enter but not on Enter alone", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    render(<ChatPanel {...base} />);
    const textarea = screen.getByLabelText("Pedido a la IA") as HTMLTextAreaElement;
    await userEvent.type(textarea, "Primera línea");
    fireEvent.keyDown(textarea, { key: "Enter", code: "Enter" });
    expect(invokeMock).not.toHaveBeenCalled();
    fireEvent.keyDown(textarea, { key: "Enter", code: "Enter", ctrlKey: true });
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith("agent_send", {
        projectId,
        prompt: "Primera línea",
        attachmentIds: [],
      }),
    );
  });

  it("toggles the material picker, selects a material, and removes it via chip", async () => {
    render(<ChatPanel {...base} />);
    expect(screen.queryByRole("button", { name: "diagrama.png" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Adjuntar material" }));
    const materialButton = screen.getByRole("button", { name: "diagrama.png" });
    expect(materialButton).toHaveAttribute("aria-pressed", "false");
    await userEvent.click(materialButton);
    expect(materialButton).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Quitar diagrama.png" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Quitar diagrama.png" }));
    expect(screen.queryByRole("button", { name: "Quitar diagrama.png" })).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Adjuntar material" }));
    expect(screen.queryByRole("button", { name: "diagrama.png" })).not.toBeInTheDocument();
  });

  it("shows the no-AI empty state, disables the composer, and opens the provider panel", async () => {
    const onOpenProvider = vi.fn();
    render(<ChatPanel {...base} aiUsable={false} onOpenProvider={onOpenProvider} />);
    expect(screen.getByText("No hay una IA conectada")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Conectar IA" })).toBeInTheDocument();
    expect(screen.getByLabelText("Pedido a la IA")).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "Conectar IA" }));
    expect(onOpenProvider).toHaveBeenCalledOnce();
  });

  it("imports an image on paste and attaches the material", async () => {
    invokeMock.mockResolvedValueOnce({
      material: materials[0],
      duplicate: false,
    });
    render(<ChatPanel {...base} />);
    const textarea = screen.getByLabelText("Pedido a la IA") as HTMLTextAreaElement;
    pasteImage(textarea);
    await waitFor(() =>
      expect(invokeMock).toHaveBeenCalledWith(
        "material_add_image",
        expect.objectContaining({
          projectId,
          fileName: "foto.png",
          contentType: "image/png",
        }),
      ),
    );
    await waitFor(() => expect(screen.getByText("diagrama.png")).toBeInTheDocument());
    expect(base.onRefresh).toHaveBeenCalled();
  });

  it("allows text-only paste without importing an image", async () => {
    render(<ChatPanel {...base} />);
    const textarea = screen.getByLabelText("Pedido a la IA") as HTMLTextAreaElement;
    fireEvent.paste(textarea, {
      clipboardData: {
        items: [{ kind: "string", type: "text/plain", getAsFile: () => null }],
        getData: () => "texto plano",
      },
    });
    expect(invokeMock).not.toHaveBeenCalled();
  });

  it("shows a working state with progress text, spinner, and cancel button", () => {
    render(<ChatPanel {...base} agentPhase="working" />);
    expect(screen.getByText("Creando tu recurso…")).toBeInTheDocument();
    expect(document.querySelector(".spinner")).toHaveAttribute("aria-hidden", "true");
    expect(screen.getByRole("button", { name: "Cancelar" })).toBeInTheDocument();
  });

  it("cancels an in-flight task", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    render(<ChatPanel {...base} agentPhase="working" />);
    await userEvent.click(screen.getByRole("button", { name: "Cancelar" }));
    expect(invokeMock).toHaveBeenCalledWith("agent_cancel", { projectId });
  });

  it("shows a failure message", () => {
    render(
      <ChatPanel {...base} agentPhase="failed" agentMessage="No se pudo completar la creación." />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("No se pudo completar la creación.");
  });

  it("renders guided error feedback instead of raw backend messages", async () => {
    invokeMock.mockRejectedValueOnce({ code: "ai_task_failed", message: "raw backend detail" });
    render(<ChatPanel {...base} />);
    const textarea = screen.getByLabelText("Pedido a la IA") as HTMLTextAreaElement;
    await userEvent.type(textarea, "Creá algo");
    await userEvent.click(screen.getByRole("button", { name: "Enviar" }));
    await waitFor(() => {
      const alert = screen.getByRole("alert");
      expect(alert).toHaveTextContent("No se pudo completar la creación.");
      expect(alert).not.toHaveTextContent("raw backend detail");
      expect(alert).not.toHaveTextContent("ai_task_failed");
    });
  });
});
