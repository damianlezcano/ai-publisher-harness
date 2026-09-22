const LOCALE = "es-AR";

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

const KIND_FALLBACK = "Archivo";
const KIND_ICONS: Record<string, string> = {
  web: "🎮",
  document: "📄",
  image: "🖼️",
  file: "📎",
  pdf: "📄",
  spreadsheet: "📊",
  presentation: "📽️",
  text: "📄",
  other: "📎",
};
const KIND_ICON_FALLBACK = "📎";
const VISIBILITY_PUBLIC = "Se compartirá";
const VISIBILITY_PRIVATE = "Privado";

export function kindLabel(kind: string): string {
  return KIND_LABELS[kind] ?? KIND_FALLBACK;
}

export function kindIcon(kind: string): string {
  return KIND_ICONS[kind] ?? KIND_ICON_FALLBACK;
}

export function visibilityLabel(visibility: string): string {
  return visibility === "public" ? VISIBILITY_PUBLIC : VISIBILITY_PRIVATE;
}

export function humanSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export function humanDate(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleString(LOCALE, {
    dateStyle: "short",
    timeStyle: "short",
  });
}

/** Compact per-turn timestamp from the persisted message timestamp. */
export function turnTimestamp(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  return date.toLocaleString(LOCALE, {
    day: "numeric",
    month: "short",
    hour: "2-digit",
    minute: "2-digit",
  });
}

const LEGACY_DEFAULT_PROJECT_NAME = /^Proyecto sin título(\s+\d+)?$/;

export function conversationDisplayName(name: string): string {
  if (LEGACY_DEFAULT_PROJECT_NAME.test(name)) {
    return messages.conversation.defaultName;
  }
  return name;
}

function importSummaryPart(count: number, singular: string, plural: string): string | null {
  if (count <= 0) return null;
  return count === 1 ? `1 ${singular}` : `${count} ${plural}`;
}

export const messages = {
  app: {
    title: "EducAI",
    loading: "Cargando…",
    settings: "Configuración",
  },

  conversations: {
    title: "Conversaciones",
    listAriaLabel: "Conversaciones",
    newButton: "Nueva conversación",
    sharedLabel: "Compartido",
    menuAriaLabel: "Opciones de conversación",
    renameLabel: "Renombrar conversación",
    renameAction: "Renombrar",
    deleteAction: "Eliminar conversación",
    deleteDisabledGenerating: "No se puede eliminar mientras se genera una respuesta.",
    deleteConfirmTitle: "¿Eliminar esta conversación?",
    deleteConfirmBody: "Se eliminarán los mensajes y los recursos asociados a esta conversación.",
    emptyTitle: "No hay conversaciones",
  },

  conversation: {
    defaultName: "Conversación nueva",
  },

  conversationDetails: {
    title: "Detalles de la conversación",
    titleAria(name: string) {
      return `Detalles de ${name}`;
    },
    conversationHeading: "Conversación",
    nameLabel: "Nombre",
    rename: "Renombrar",
    modelHeading: "Modelo",
    modelLabel: "Modelo de esta conversación",
    globalDefault: "Predeterminado de Configuración",
    activeTurnNotice: "Esperá a que termine la solicitud antes de cambiar el modelo.",
    filesHeading: "Archivos y material",
    uploadedHeading: "Material subido",
    noUploaded: "No hay material subido.",
    openContainingFolder: "Abrir carpeta contenedora",
    generatedHeading: "Creaciones generadas",
    noGenerated: "No hay creaciones generadas.",
    metrics: {
      heading: "Uso y optimización",
      lastTurn: "Último turno",
      accumulated: "Acumulado de la conversación",
      accumulatedProvider: "Uso real acumulado del proveedor",
      realProvider: "Uso real del proveedor",
      knowledgeOptimization: "Optimización Knowledge (estimaciones locales)",
      unavailable: "No disponible",
      provider: "Proveedor",
      model: "Modelo",
      inputTokens: "Tokens de entrada",
      outputTokens: "Tokens de salida",
      cacheReadTokens: "Tokens de caché leídos",
      cacheWriteTokens: "Tokens de caché escritos",
      actualCost: "Costo real informado",
      turnDuration: "Duración del turno",
      remoteCalls: "Llamadas al proveedor",
      reason: "Motivo de la llamada",
      additionalAttachmentRoute:
        "El proveedor recibió contenido por una ruta adicional de adjuntos.",
      materialCount: "Materiales del proyecto",
      corpusBytes: "Corpus",
      corpusChars: "Corpus (caracteres)",
      naiveCorpusTokens: "Corpus estimado",
      candidateCount: "Candidatos de recuperación",
      selectedEvidenceCount: "Evidencias seleccionadas",
      selectedEvidenceBytes: "Evidencias seleccionadas",
      selectedEvidenceChars: "Evidencias (caracteres)",
      evidenceTokens: "Contexto enviado",
      contextReduction: "Reducción estimada de contexto",
      retrievalMode: "Modo de recuperación",
      localMode: "Modo local",
      estimateNotice:
        "Estas son estimaciones locales de contexto de Knowledge, no tokens facturados ni un ahorro exacto.",
    },
  },

  sessionLogs: {
    heading: "Logs de esta sesión",
    description:
      "Información de EducAI durante esta ejecución. No incluye contenido de tus archivos ni mensajes.",
    clear: "Limpiar",
    copy: "Copiar",
    refresh: "Actualizar",
    empty: "Sin eventos todavía.",
    latestAnnouncement(level: string) {
      return `Nuevo evento de nivel ${level}.`;
    },
  },

  common: {
    cancel: "Cancelar",
    close: "Cerrar",
    create: "Crear",
    save: "Guardar",
    open: "Abrir",
    delete: "Eliminar",
    remove: "Quitar",
    send: "Enviar",
    retry: "Reintentar",
    confirm: "Confirmar",
    confirmYes: "Sí",
    confirmPrompt: "Para confirmar, escribí Sí.",
    confirmNameLabel: "Confirmación",
    copied: "Copiado",
  },

  project: {
    listHeading: "Mis proyectos",
    newButton: "Nuevo proyecto",
    backToList: "← Proyectos",
    listAriaLabel: "Proyectos",
    nameLabel: "Nombre del proyecto",
    namePlaceholder: "Nombre del proyecto",
    renameLabel: "Nuevo nombre",
    open: "Abrir",
    rename: "Renombrar",
    defaultName: "Proyecto sin título",
    empty: {
      title: "Todavía no tenés proyectos",
      action: "Crear proyecto",
    },
    delete: {
      title: "Eliminar proyecto",
      confirmMessage(name: string): string {
        return `Escribí “${name}” para confirmar la eliminación.`;
      },
    },
    firstRun: {
      title: "Empezá con EducAI",
      steps: [
        "Creá un proyecto",
        "Agregá material",
        "Pedile a la IA que cree algo",
        "Mirá la creación",
        "Compartila con tus estudiantes",
      ],
      dismiss: "Entendido",
    },
    closeWarning: "Si cerrás la aplicación, los enlaces compartidos dejarán de funcionar.",
  },

  assistant: {
    panelLabel: "Asistente",
    heading: "Asistente",
    emptyHint: "Escribí un mensaje o pedí algo.",
    promptLabel: "Pedido a la IA",
    placeholder: "Escribí un mensaje o pedí algo...",
    attachmentsAriaLabel: "Archivos adjuntos",
    attachMaterial: "Adjuntar",
    attachmentFallback: "Archivo adjunto",
    selectedCount(count: number): string {
      return `${count} archivos seleccionados`;
    },
    showAll: "Ver todos",
    hideAll: "Ver menos",
    starting: "Preparando el asistente…",
    removeAttachment(name: string): string {
      return `Quitar ${name}`;
    },
  },

  turnMetrics: {
    infoAria: "Detalles de esta respuesta",
    detailsLabel: "Detalles de esta respuesta",
    // Compact line segment for the local Knowledge context reduction.
    knowledgeReduction(percent: string): string {
      return `Knowledge −${percent}`;
    },
    // Detailed popover sections and fields (shared labels are reused from
    // `conversationDetails.metrics`; these are the per-turn specific ones).
    responseHeading: "Respuesta",
    dateTime: "Fecha y hora",
    knowledgeHeading: "Knowledge",
    contextSentEstimated: "Contexto enviado (estimado)",
    contextReduction: "Reducción de contexto",
    sourcesHeading: "Fuentes utilizadas",
  },

  timeline: {
    userLabel: "Vos",
    assistantLabel: "Asistente",
    resourceLabel: "Material",
    unattachedTitle: "Materiales",
    collapse: "Ocultar",
    expand: "Mostrar",
    attachmentsSummary(count: number): string {
      return `📎 ${count} archivos adjuntos`;
    },
    showAttachments: "Ver archivos",
    hideAttachments: "Ocultar archivos",
  },

  agent: {
    creating: "Procesando tu solicitud…",
    taskFailed: "No se pudo completar la creación.",
  },

  material: {
    panelLabel: "Materiales",
    heading: "Materiales",
    kindFallback: KIND_FALLBACK,
    addFile: "Agregar archivo",
    dropOverlay: "Soltá los archivos acá",
    empty: {
      title: "Agregá material para darle contexto a la IA",
      pasteHint: "o pegá una imagen con Ctrl+V",
    },
    legacyEmpty: "Arrastrá archivos acá o usá “Agregar archivo”.",
    importing: "Agregando archivos…",
    duplicateSingle: "Ese archivo ya está en el proyecto.",
    importPartialFailure: "No pudimos agregar algunos archivos.",
    importSummary(added: number, duplicate: number, failed: number): string {
      const parts = [
        importSummaryPart(added, "agregado", "agregados"),
        importSummaryPart(duplicate, "ya estaba", "ya estaban"),
        importSummaryPart(failed, "no se pudo agregar", "no se pudieron agregar"),
      ].filter((part): part is string => part !== null);
      return parts.join(" · ");
    },
    perFileAdded(name: string): string {
      return `Se agregó ${name}.`;
    },
    perFileDuplicate(name: string): string {
      return `${name} ya estaba en el proyecto.`;
    },
    perFileDuplicateInBatch(name: string): string {
      return `${name} ya estaba en esta selección.`;
    },
    perFileFailed(name: string): string {
      return `No se pudo agregar ${name}.`;
    },
    removeConfirm(name: string): string {
      return `¿Quitar ${name}?`;
    },
    removeConfirmAriaLabel: "Confirmar eliminación",
  },

  creation: {
    panelLabel: "Creaciones",
    heading: "Creaciones",
    preview: "Vista previa",
    empty: {
      title: "Pedile a la IA que cree algo",
      hint: "Escribí en el asistente",
    },
    legacyEmpty: "Todavía no hay creaciones. Pedile algo a la IA.",
    previewLoading: "Abriendo vista previa…",
    previewAriaLabel(title: string): string {
      return `Vista previa: ${title}`;
    },
  },

  preview: {
    binaryHint: "No podemos previsualizar este tipo de archivo.",
    openExternal: "Abrir con la aplicación",
  },

  sharing: {
    panelLabel: "Compartir",
    shared: "Compartido",
    shareAction: "Compartir",
    sharing: "Compartiendo…",
    stopSharing: "Dejar de compartir",
    stopping: "Dejando de compartir…",
    copyLink: "Copiar enlace",
    openLink: "Abrir enlace",
    showQr: "Mostrar QR",
    copyLinkFailed: "No pudimos copiar el enlace.",
    temporaryNote:
      "Este enlace funciona mientras el recurso esté compartido. Si cerrás la aplicación, dejás de compartir o se corta la conexión, el enlace deja de funcionar.",
    stopConfirm: {
      title: "Dejar de compartir",
      message: "Si dejás de compartir, tus estudiantes ya no podrán abrir el enlace.",
    },
  },

  qr: {
    title: "Código QR",
    generating: "Generando código QR…",
    generateFailed: "No pudimos generar el código QR.",
    altForProject(projectName: string, url: string): string {
      return `Código QR de ${projectName} para ${url}`;
    },
    altForUrl(url: string): string {
      return `Código QR del enlace ${url}`;
    },
  },

  provider: {
    panelLabel: "Configuración",
    heading: "Configuración",
    tabs: {
      general: "General",
      logs: "Logs",
    },
    privacyNote:
      "Tu cuenta y tus claves se guardan de forma segura en tu computadora. Nunca se comparten.",
    featuredHeading: "Recomendados",
    noFeatured: "Aún no hay proveedores destacados.",
    othersButton(count: number): string {
      return `Otros proveedores (${count})`;
    },
    searchPlaceholder: "Buscar proveedor",
    searchAriaLabel: "Buscar proveedor",
    noSearchResults: "No encontramos proveedores.",
    connect: "Conectar",
    reconnect: "Conectar de nuevo",
    hide: "Ocultar",
    connected: "Conectado",
    connectedNotice: "Conectado.",
    accountConnected: "Cuenta conectada.",
    disconnected: "Desconectado.",
    disconnect: "Desconectar",
    testConnection: "Probar conexión",
    testing: "Comprobando conexión…",
    connecting: "Conectando…",
    oauthInstructions: "Abrí el enlace y aprobá el acceso.",
    oauthOpenBrowser: "Abrir en el navegador",
    verificationCodeLabel: "Código de verificación",
    verificationCodePlaceholder: "Código de verificación",
    complete: "Completar",
    oauthFailed: "No pudimos completar la conexión. Intentalo de nuevo.",
    disconnectConfirm: {
      title: "Desconectar",
      message:
        "Si desconectás, vas a necesitar volver a conectar tu cuenta para usar modelos de pago.",
    },
    banner: {
      freeModel: "Modelo gratuito",
      noAiConnected: "No hay una IA conectada. Conectá tu cuenta para seguir creando.",
      reconnectRequired: "Necesitás volver a conectar tu cuenta.",
      connectAction: "Conectar IA",
    },
  },

  model: {
    label: "Modelo",
    loading: "Cargando…",
    none: "Sin modelos",
    choose: "Elegí un modelo",
    free: "Gratis",
    paid: "De pago",
    freeSuffix: " / Gratis",
    paidSuffix: " / De pago",
    automaticFree: "Modelo automático · Gratis",
    groupRecommended: "Recomendado",
    groupFree: "Gratis",
    unavailableChoice: "Este modelo ya no está disponible. Elegí otro.",
    freeModelsNotice: "Los modelos gratis pueden cambiar con el tiempo.",
  },

  progress: {
    importing: "Agregando archivos…",
    sharing: "Compartiendo…",
    sharingNote: "puede tardar unos segundos",
    connecting: "Conectando…",
    testingConnection: "Comprobando conexión…",
    openingPreview: "Abriendo vista previa…",
    generatingQr: "Generando código QR…",
  },

  processing: {
    semanticUnavailable: "Indexación semántica no disponible",
    pendingRetry: "El procesamiento quedó pendiente. Podés reintentarlo.",
    resumeFailed: "No pudimos reanudar el procesamiento local.",
    noTurn: "El envío se interrumpió antes de confirmarse; volvé a enviarlo.",
    outcomeUnknown:
      "El resultado anterior quedó pendiente y no se puede continuar automáticamente.",
    cannotContinue: "No pudimos continuar el procesamiento local.",
  },
  progressDetails: {
    infoAria: "Detalles del procesamiento",
    readyLabel: "Archivos listos",
    errorsLabel: "Archivos con error",
    preparedLabel: "Preparación",
    fragmentsLabel: "Fragmentos procesados",
    embeddingsLabel: "Embeddings completados",
    embeddingsCreatedLabel: "Embeddings creados",
    embeddingsReusedLabel: "Embeddings reutilizados",
    elapsedLabel: "Tiempo transcurrido",
    speedLabel: "Velocidad",
  },
  compactProgress: {
    /** Prefix for the single merged in-flight line ("Procesando tu solicitud · "). */
    requestPrefix: "Procesando tu solicitud · ",
    embeddingPercent(percent: number): string {
      return `${percent}%`;
    },
    readyCount(ready: number, total: number): string {
      return total === 1 ? `${ready} de 1 archivo listo` : `${ready} de ${total} archivos listos`;
    },
    readyCountWithErrors(ready: number, total: number, errors: number): string {
      const base =
        total === 1 ? `${ready} de 1 archivo listo` : `${ready} de ${total} archivos listos`;
      return `${base} · ${errors} con error`;
    },
    readyTotal(count: number): string {
      return count === 1 ? "1 archivo listo" : `${count} archivos listos`;
    },
    /** Truthful synthesis phase shown once embeddings complete and compact
     * summary generation starts (a single indeterminate phase for one-call
     * synthesis). Never fakes per-file provider progress. */
    generatingSummaries(count: number): string {
      return count === 1 ? "Generando resumen…" : `Generando resúmenes de ${count} archivos…`;
    },
    /** Indeterminate synthesis label for a no-attachment compact follow-up,
     * where the backend's single aggregate call exposes no per-item count. */
    generatingSummariesIndeterminate: "Generando resúmenes…",
  },

  error: {
    actionRetry: "Reintentar",
    actionConnectAi: "Conectar IA",
    actionOpenWithApp: "Abrir con la aplicación",
    aiUnavailable: {
      title: "El asistente no pudo iniciarse.",
      message: "El asistente no pudo iniciarse.",
      hint: "si persiste, reiniciá la aplicación",
    },
    aiTaskFailed: {
      title: "No se pudo completar la creación.",
      message: "No se pudo completar la creación.",
    },
    publishFailed: {
      title: "No pudimos compartir en este momento.",
      message: "No pudimos compartir en este momento.",
      hint: "comprobá tu conexión a Internet",
    },
    networkError: {
      title: "No hay conexión a Internet.",
      message: "No hay conexión a Internet.",
    },
    materialFailed: {
      title: "No pudimos agregar ese archivo.",
      message: "No pudimos agregar ese archivo.",
    },
    materialUnsupported: {
      title: "No admitimos ese tipo de archivo.",
      message: "No admitimos ese tipo de archivo.",
    },
    materialDuplicate: {
      title: "Ese archivo ya está en el proyecto.",
      message: "Ese archivo ya está en el proyecto.",
    },
    previewUnavailable: {
      title: "No pudimos mostrar la vista previa.",
      message: "No pudimos mostrar la vista previa.",
    },
    previewTooLarge: {
      title: "Este recurso es grande.",
      message: "Este recurso es grande.",
    },
    credentialRevoked: {
      title: "Necesitás volver a conectar tu cuenta.",
      message: "Necesitás volver a conectar tu cuenta.",
    },
    credentialInvalid: {
      title: "Necesitás volver a conectar tu cuenta.",
      message: "Necesitás volver a conectar tu cuenta.",
    },
    providerUnavailable: {
      title: "El proveedor no está disponible.",
      message: "Conectá una IA para seguir creando.",
    },
    noCompatibleModel: {
      title: "No hay un modelo compatible.",
      message: "Conectá una IA o elegí otro modelo.",
    },
    modelUnavailable: {
      title: "El modelo no está disponible.",
      message: "Elegí otro modelo o conectá una IA.",
    },
    openFailed: {
      title: "No pudimos abrir el recurso.",
      message: "No pudimos abrir el recurso.",
    },
    storageUnavailable: {
      title: "Algo salió mal.",
      message: "Algo salió mal.",
      hint: "reiniciá la aplicación",
    },
    internal: {
      title: "Algo salió mal.",
      message: "Algo salió mal.",
      hint: "reiniciá la aplicación",
    },
    refreshAfterSend: {
      title: "El mensaje ya se envió.",
      message: "No pudimos actualizar la vista, pero tu mensaje está confirmado.",
    },
  },
} as const;

export function formatElapsed(ms: number): string {
  const totalSeconds = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) return `${hours}h ${minutes}m ${seconds}s`;
  if (minutes > 0) return `${minutes}m ${seconds}s`;
  return `${seconds}s`;
}
