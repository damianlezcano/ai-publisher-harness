import { beforeEach, describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import PublishPanel from "./PublishPanel";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

const invokeMock = vi.mocked(invoke);

const projectId = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22";
const url = "https://fake.trycloudflare.com/fotosintesis-a7k2m9/";

beforeEach(() => {
  invokeMock.mockReset();
});

describe("PublishPanel", () => {
  it("publishes and refreshes", async () => {
    const onRefresh = vi.fn();
    invokeMock.mockResolvedValueOnce({ state: "published", publicUrl: url });
    render(
      <PublishPanel
        projectId={projectId}
        publication={{ state: "local", publicUrl: null }}
        onRefresh={onRefresh}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Compartir" }));
    expect(invokeMock).toHaveBeenCalledWith("publish", { projectId });
    await waitFor(() => expect(onRefresh).toHaveBeenCalled());
  });

  it("shows the public URL with actions when published", () => {
    render(
      <PublishPanel
        projectId={projectId}
        publication={{ state: "published", publicUrl: url }}
        onRefresh={() => {}}
      />,
    );
    expect(screen.getByLabelText("Enlace para compartir")).toHaveTextContent(url);
    expect(screen.getByRole("button", { name: "Copiar enlace" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Mostrar QR" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Dejar de compartir" })).toBeInTheDocument();
  });

  it("shows a human error when publish fails", async () => {
    invokeMock.mockRejectedValueOnce({
      code: "publish_failed",
      message: "No se pudo publicar en Internet.",
    });
    render(
      <PublishPanel
        projectId={projectId}
        publication={{ state: "local", publicUrl: null }}
        onRefresh={() => {}}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Compartir" }));
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent("No se pudo publicar en Internet."),
    );
  });

  it("unpublishes and refreshes", async () => {
    const onRefresh = vi.fn();
    invokeMock.mockResolvedValueOnce({ state: "local", publicUrl: null });
    render(
      <PublishPanel
        projectId={projectId}
        publication={{ state: "published", publicUrl: url }}
        onRefresh={onRefresh}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Dejar de compartir" }));
    expect(invokeMock).toHaveBeenCalledWith("unpublish", { projectId });
    await waitFor(() => expect(onRefresh).toHaveBeenCalled());
  });

  it("shows the QR dialog from the public URL", async () => {
    render(
      <PublishPanel
        projectId={projectId}
        publication={{ state: "published", publicUrl: url }}
        onRefresh={() => {}}
      />,
    );
    await userEvent.click(screen.getByRole("button", { name: "Mostrar QR" }));
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });
});
