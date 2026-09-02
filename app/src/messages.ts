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
    starting: "Preparando el asistente…",
    removeAttachment(name: string): string {
      return `Quitar ${name}`;
    },
  },

  timeline: {
    userLabel: "Vos",
    assistantLabel: "Asistente",
    resourceLabel: "Material",
    unattachedTitle: "Materiales",
    collapse: "Ocultar",
    expand: "Mostrar",
  },

  agent: {
    creating: "Creando tu recurso…",
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
    creating: "Creando tu recurso…",
    importing: "Agregando archivos…",
    sharing: "Compartiendo…",
    sharingNote: "puede tardar unos segundos",
    connecting: "Conectando…",
    testingConnection: "Comprobando conexión…",
    openingPreview: "Abriendo vista previa…",
    generatingQr: "Generando código QR…",
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
  },
} as const;
