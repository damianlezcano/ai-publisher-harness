import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ProviderStatusBanner from "./ProviderStatusBanner";

describe("ProviderStatusBanner", () => {
  it("free shows the free-model badge and never claims no AI is connected", () => {
    render(<ProviderStatusBanner status="free" onConnect={vi.fn()} />);
    expect(screen.getByText("Modelo gratuito")).toHaveClass("badge ok");
    expect(screen.queryByText("Conectar IA")).not.toBeInTheDocument();
    expect(screen.queryByText(/No hay una IA conectada/)).not.toBeInTheDocument();
    expect(screen.queryByText(/volver a conectar/)).not.toBeInTheDocument();
  });

  it("requires-choice shows the guidance and a connect action", async () => {
    const user = userEvent.setup();
    const onConnect = vi.fn();
    render(<ProviderStatusBanner status="requires-choice" onConnect={onConnect} />);
    expect(screen.getByText(/No hay una IA conectada/)).toBeInTheDocument();
    const connect = screen.getByRole("button", { name: "Conectar IA" });
    expect(connect).toHaveClass("primary");
    await user.click(connect);
    expect(onConnect).toHaveBeenCalledTimes(1);
  });

  it("needs-reconnect shows the reconnect guidance and a connect action", () => {
    const onConnect = vi.fn();
    render(<ProviderStatusBanner status="needs-reconnect" onConnect={onConnect} />);
    expect(screen.getByText("Necesitás volver a conectar tu cuenta.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Conectar IA" })).toBeInTheDocument();
  });
});
