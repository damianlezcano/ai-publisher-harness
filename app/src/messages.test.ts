import { describe, expect, it } from "vitest";
import { humanDate, humanSize, kindLabel, messages, visibilityLabel } from "./messages";

const FORBIDDEN_TERMS = [
  "Cloudflare",
  "OpenCode",
  "tunnel",
  "puerto",
  "localhost",
  "Quick Tunnel",
  "API",
  "server",
  "hosting",
  "runtime",
  "DNS",
  "Publicar",
  "Publicado",
] as const;

function collectStrings(value: unknown, out: string[] = []): string[] {
  if (typeof value === "string") {
    out.push(value);
    return out;
  }
  if (typeof value === "function") {
    return out;
  }
  if (value && typeof value === "object") {
    for (const entry of Object.values(value)) {
      collectStrings(entry, out);
    }
  }
  return out;
}

function catalogStrings(): string[] {
  const staticStrings = collectStrings(messages);
  const dynamicStrings = [
    messages.project.delete.confirmMessage("Proyecto demo"),
    messages.assistant.removeAttachment("archivo.pdf"),
    messages.material.importSummary(3, 1, 1),
    messages.material.perFileAdded("manual.pdf"),
    messages.material.perFileDuplicate("manual.pdf"),
    messages.material.perFileFailed("bad.exe"),
    messages.material.removeConfirm("manual.pdf"),
    messages.provider.othersButton(2),
    messages.qr.altForProject("Fotosíntesis", "https://example.test/share"),
    messages.qr.altForUrl("https://example.test/share"),
    messages.creation.previewAriaLabel("Notas"),
  ];
  return [...staticStrings, ...dynamicStrings];
}

describe("messages catalog", () => {
  it("uses canonical project terminology", () => {
    expect(messages.project.listHeading).toBe("Mis proyectos");
    expect(messages.project.newButton).toBe("Nuevo proyecto");
  });

  it("uses canonical assistant terminology", () => {
    expect(messages.assistant.heading).toBe("Asistente");
    expect(messages.assistant.panelLabel).toBe("Asistente");
  });

  it("uses canonical sharing terminology", () => {
    expect(messages.sharing.shareAction).toBe("Compartir");
    expect(messages.sharing.sharing).toBe("Compartiendo…");
    expect(messages.sharing.shared).toBe("Compartido");
    expect(messages.sharing.notShared).toBe("No compartido");
    expect(messages.sharing.linkLabel).toBe("Enlace para compartir");
    expect(messages.sharing.copyLink).toBe("Copiar enlace");
    expect(messages.sharing.openLink).toBe("Abrir enlace");
    expect(messages.sharing.showQr).toBe("Mostrar QR");
    expect(messages.sharing.stopSharing).toBe("Dejar de compartir");
    expect(messages.sharing.stopped).toBe("Dejaste de compartirlo");
    expect(messages.sharing.empty.title).toBe("Este proyecto todavía no se comparte");
  });

  it("includes the exact temporary-link note", () => {
    expect(messages.sharing.temporaryNote).toBe(
      "Este enlace funciona mientras el recurso esté compartido. Si cerrás la aplicación, dejás de compartir o se corta la conexión, el enlace deja de funcionar.",
    );
  });

  it("never includes forbidden technical or legacy publish terms in catalog values", () => {
    const values = catalogStrings();
    for (const value of values) {
      for (const term of FORBIDDEN_TERMS) {
        expect(value.toLowerCase()).not.toContain(term.toLowerCase());
      }
    }
  });

  it("formats material import summaries", () => {
    expect(messages.material.importSummary(3, 1, 1)).toBe(
      "3 agregados · 1 ya estaba · 1 no se pudo agregar",
    );
    expect(messages.material.importSummary(1, 0, 0)).toBe("1 agregado");
    expect(messages.material.importSummary(0, 2, 0)).toBe("2 ya estaban");
  });

  it("maps kinds and visibility through helpers", () => {
    expect(kindLabel("web")).toBe("Actividad interactiva");
    expect(kindLabel("unknown")).toBe("Archivo");
    expect(visibilityLabel("public")).toBe("Se compartirá");
    expect(visibilityLabel("private")).toBe("Privado");
  });

  it("formats sizes and dates with es-AR locale", () => {
    expect(humanSize(512)).toBe("512 B");
    expect(humanSize(2048)).toBe("2 KB");
    expect(humanDate("not-a-date")).toBe("not-a-date");
    expect(humanDate("2026-08-28T15:00:00Z")).toMatch(/\d/);
  });
});
