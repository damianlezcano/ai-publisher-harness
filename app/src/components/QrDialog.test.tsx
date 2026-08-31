import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Mock } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import QRCode from "qrcode";
import QrDialog from "./QrDialog";
import { messages } from "../messages";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("qrcode", () => ({
  default: { toDataURL: vi.fn() },
}));

const invokeMock = vi.mocked(invoke);
const toDataURLMock = QRCode.toDataURL as unknown as Mock;

const projectId = "0198e4a6-6e70-7c01-8c0e-8b6fd26f1f22";
const projectName = "Fotosíntesis";
const url = "https://fake.trycloudflare.com/fotosintesis-a7k2m9/";

function renderDialog(onClose = vi.fn()) {
  return render(
    <QrDialog projectId={projectId} url={url} projectName={projectName} onClose={onClose} />,
  );
}

beforeEach(() => {
  invokeMock.mockReset();
  toDataURLMock.mockReset();
});

describe("QrDialog", () => {
  it("shows the project name as heading and the generating message while loading", () => {
    toDataURLMock.mockReturnValue(new Promise(() => {}));
    renderDialog();
    expect(screen.getByRole("heading", { name: projectName })).toBeInTheDocument();
    expect(screen.getByText(messages.qr.generating)).toBeInTheDocument();
  });

  it("renders the QR image with the project-based alt text", async () => {
    toDataURLMock.mockResolvedValue("data:image/png;base64,FAKE");
    renderDialog();
    const img = await screen.findByRole("img");
    expect(img).toHaveAttribute("alt", messages.qr.altForProject(projectName, url));
    expect(toDataURLMock).toHaveBeenCalledWith(url, { width: 360, margin: 1 });
  });

  it("copies the link, opens it, and closes", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });
    toDataURLMock.mockResolvedValue("data:image/png;base64,FAKE");
    invokeMock.mockResolvedValueOnce(undefined);
    renderDialog(onClose);

    await user.click(screen.getByRole("button", { name: messages.sharing.copyLink }));
    expect(writeText).toHaveBeenCalledWith(url);
    expect(screen.getByRole("button", { name: messages.common.copied })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: messages.sharing.openLink }));
    expect(invokeMock).toHaveBeenCalledWith("open_public_url", { projectId });

    await user.click(screen.getByRole("button", { name: messages.common.close }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("shows an error when QR generation fails", async () => {
    toDataURLMock.mockRejectedValue(new Error("boom"));
    renderDialog();
    await waitFor(() =>
      expect(screen.getByRole("alert")).toHaveTextContent(messages.qr.generateFailed),
    );
  });

  it("closes on Escape", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    toDataURLMock.mockResolvedValue("data:image/png;base64,FAKE");
    renderDialog(onClose);
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
