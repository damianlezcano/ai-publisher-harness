import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import ChatPanel from "./ChatPanel";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

const base = {
  projectId: "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22",
  agentPhase: "idle" as const,
  agentMessage: null as string | null,
  onRefresh: () => {},
};

beforeEach(() => {
  invokeMock.mockReset();
});

describe("ChatPanel", () => {
  it("sends a prompt and clears the input", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    render(<ChatPanel {...base} />);
    await userEvent.type(screen.getByLabelText("Pedido a la IA"), "Creá una actividad");
    await userEvent.click(screen.getByRole("button", { name: "Enviar" }));
    expect(invokeMock).toHaveBeenCalledWith("agent_send", {
      projectId: base.projectId,
      prompt: "Creá una actividad",
    });
    expect(screen.getByLabelText("Pedido a la IA")).toHaveValue("");
  });

  it("shows a working state with a cancel button", () => {
    render(<ChatPanel {...base} agentPhase="working" />);
    expect(screen.getByText("Creando tu recurso…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Cancelar" })).toBeInTheDocument();
  });

  it("cancels an in-flight task", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    render(<ChatPanel {...base} agentPhase="working" />);
    await userEvent.click(screen.getByRole("button", { name: "Cancelar" }));
    expect(invokeMock).toHaveBeenCalledWith("agent_cancel", { projectId: base.projectId });
  });

  it("shows a failure message", () => {
    render(
      <ChatPanel {...base} agentPhase="failed" agentMessage="No se pudo completar la creación." />,
    );
    expect(screen.getByRole("alert")).toHaveTextContent("No se pudo completar la creación.");
  });
});
