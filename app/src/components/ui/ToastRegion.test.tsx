import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import ToastRegion from "./ToastRegion";
import { useToast } from "./useToast";

function ToastHarness() {
  const { toasts, show, dismiss } = useToast();
  return (
    <>
      <ToastRegion toasts={toasts} />
      <button type="button" onClick={() => show("Tu recurso está listo.")}>
        Mostrar
      </button>
      {toasts.map((toast) => (
        <button key={toast.id} type="button" onClick={() => dismiss(toast.id)}>
          Descartar {toast.id}
        </button>
      ))}
    </>
  );
}

describe("ToastRegion", () => {
  it("renders a polite live region", () => {
    render(<ToastRegion toasts={[]} />);
    const region = screen.getByRole("status");
    expect(region).toHaveAttribute("aria-live", "polite");
  });

  it("renders each toast inside the live region", () => {
    render(
      <ToastRegion
        toasts={[
          { id: "a", children: "Listo." },
          { id: "b", children: "Compartido." },
        ]}
      />,
    );
    const region = screen.getByRole("status");
    expect(region).toHaveTextContent("Listo.");
    expect(region).toHaveTextContent("Compartido.");
  });
});

describe("useToast", () => {
  it("shows and dismisses toasts", async () => {
    const user = userEvent.setup();
    render(<ToastHarness />);
    expect(screen.queryByText("Tu recurso está listo.")).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Mostrar" }));
    expect(screen.getByText("Tu recurso está listo.")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /^Descartar/ }));
    expect(screen.queryByText("Tu recurso está listo.")).not.toBeInTheDocument();
  });
});
