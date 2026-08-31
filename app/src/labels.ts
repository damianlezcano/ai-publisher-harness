const KIND_LABELS: Record<string, string> = {
  web: "Actividad interactiva",
  document: "Documento",
  image: "Imagen",
  file: "Archivo",
  pdf: "Documento PDF",
  spreadsheet: "Hoja de cálculo",
  presentation: "Presentación",
  text: "Texto",
  other: "Archivo",
};

export function kindLabel(kind: string): string {
  return KIND_LABELS[kind] ?? "Archivo";
}

export function visibilityLabel(visibility: string): string {
  return visibility === "public" ? "Se compartirá" : "Privado";
}

export function humanSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function humanDate(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleString("es-AR", {
    dateStyle: "short",
    timeStyle: "short",
  });
}
