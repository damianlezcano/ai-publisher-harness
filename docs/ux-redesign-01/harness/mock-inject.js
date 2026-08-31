(() => {
  // Backend state ----------------------------------------------------------
  function makeId(prefix) {
    return prefix + "-" + Math.random().toString(36).slice(2, 10);
  }
  const now = Date.now();
  function iso(offsetMs) {
    return new Date(now + offsetMs).toISOString();
  }

  const FREE_MODEL = {
    providerId: "opencode",
    modelId: "big-pickle",
    name: "big-pickle",
    free: true,
    recommended: true,
    deprecated: false,
  };

  const PROVIDER_GRATIS = {
    id: "opencode",
    name: "Gratis",
    authMethods: [
      {
        kind: "api_key",
        methodId: null,
        label: "Clave de API",
        prompts: [],
        placeholder: null,
        optional: false,
      },
    ],
    connected: false,
    connectionLabel: null,
    highlighted: true,
  };

  let seedName = "first-launch";
  let projects = [];
  let selectedModel = { model: FREE_MODEL, notice: null, requiresChoice: false };
  let modelList = [FREE_MODEL];
  let providerList = [PROVIDER_GRATIS];

  function storageKey(name) {
    return "__UX_MOCK_" + name;
  }

  function persist() {
    try {
      localStorage.setItem(
        storageKey(seedName),
        JSON.stringify({ projects, selectedModel, modelList, providerList }),
      );
    } catch {}
  }

  function loadPersisted(name) {
    try {
      const raw = localStorage.getItem(storageKey(name));
      if (raw) {
        const parsed = JSON.parse(raw);
        if (parsed && Array.isArray(parsed.projects)) {
          projects = parsed.projects;
          selectedModel = parsed.selectedModel || selectedModel;
          modelList = parsed.modelList || modelList;
          providerList = parsed.providerList || providerList;
          return true;
        }
      }
    } catch {}
    return false;
  }

  function clearPersisted(name) {
    try {
      localStorage.removeItem(storageKey(name));
    } catch {}
  }

  function makeSummary(p) {
    return {
      id: p.id,
      name: p.name,
      createdAt: p.createdAt,
      updatedAt: p.updatedAt,
      shared: p.publication?.state === "published" && !!p.publication?.publicUrl,
    };
  }

  function makeProjectView(p) {
    return {
      id: p.id,
      name: p.name,
      materials: [...p.materials],
      creations: [...p.creations],
      publication: { ...p.publication },
      messages: [...(p.messages || [])],
    };
  }

  function seed(name) {
    seedName = name || "first-launch";

    // Restore durable per-seed state if it exists.
    if (loadPersisted(seedName)) {
      return;
    }

    projects = [];
    selectedModel = { model: FREE_MODEL, notice: null, requiresChoice: false };
    modelList = [FREE_MODEL];
    providerList = [PROVIDER_GRATIS];

    if (name === "list") {
      const p = (n, off) => ({
        id: "proj-" + n.toLowerCase().replace(/\s+/g, "-"),
        name: n,
        createdAt: iso(off),
        updatedAt: iso(off),
        messages: [],
      });
      projects = [
        {
          ...p("Fracciones", 0),
          materials: [],
          creations: [],
          publication: { state: "local", publicUrl: null },
        },
        {
          ...p("Sistema solar", -3600000),
          materials: [],
          creations: [],
          publication: { state: "local", publicUrl: null },
        },
        {
          ...p("Fotosíntesis", -7200000),
          materials: [],
          creations: [],
          publication: {
            state: "published",
            publicUrl: "https://educai-demo-7k2x.trycloudflare.com/fotosintesis-a7k2",
          },
        },
      ];
    }

    if (name === "workspace" || name === "shared") {
      const p = (n, off) => ({
        id: "proj-" + n.toLowerCase().replace(/\s+/g, "-"),
        name: n,
        createdAt: iso(off),
        updatedAt: iso(off),
        messages: [],
      });
      projects = [
        {
          ...p("Fracciones", -7200000),
          materials: [],
          creations: [],
          publication: { state: "local", publicUrl: null },
        },
        {
          ...p("Sistema solar", -3600000),
          materials: [],
          creations: [],
          publication: { state: "local", publicUrl: null },
        },
        {
          ...p("Fotosíntesis", 0),
          materials: [],
          creations: [],
          publication: { state: "local", publicUrl: null },
        },
      ];
      const fotos = projects.find((x) => x.name === "Fotosíntesis");
      if (fotos) {
        fotos.materials = [
          {
            id: "mat-1",
            displayName: "manual.pdf",
            originalFileName: "manual.pdf",
            kind: "pdf",
            byteSize: 51200,
            createdAt: iso(-7000000),
          },
          {
            id: "mat-2",
            displayName: "esquema-fotosíntesis.png",
            originalFileName: "esquema-fotosíntesis.png",
            kind: "image",
            byteSize: 204800,
            createdAt: iso(-6900000),
          },
          {
            id: "mat-3",
            displayName: "diapo.pptx",
            originalFileName: "diapo.pptx",
            kind: "presentation",
            byteSize: 102400,
            createdAt: iso(-6850000),
          },
        ];
        fotos.creations = [
          {
            id: "cre-1",
            displayName: "Actividad interactiva: fotosíntesis",
            kind: "web",
            visibility: "public",
            byteSize: 409600,
            createdAt: iso(-6800000),
            revision: 1,
          },
          {
            id: "cre-2",
            displayName: "Guía de trabajo (PDF)",
            kind: "pdf",
            visibility: "private",
            byteSize: 24576,
            createdAt: iso(-6700000),
            revision: 1,
          },
        ];
        fotos.messages = [
          {
            id: "msg-user-1",
            role: "user",
            text: "Creá una actividad sobre fotosíntesis usando el manual y el esquema.",
            status: "ok",
            createdAt: iso(-6500000),
            materialIds: ["mat-1", "mat-2"],
            creationIds: [],
          },
          {
            id: "msg-assistant-1",
            role: "assistant",
            text: "Listo.",
            status: "ok",
            createdAt: iso(-6400000),
            materialIds: [],
            creationIds: ["cre-1"],
          },
        ];
        if (name === "shared") {
          fotos.publication = {
            state: "published",
            publicUrl: "https://educai-demo-7k2x.trycloudflare.com/fotosintesis-a7k2",
          };
        }
      }
    }

    if (name === "disconnected") {
      selectedModel = { model: null, notice: "No hay modelos disponibles.", requiresChoice: true };
      modelList = [];
      providerList = [];
      projects = [
        {
          id: "proj-Fotosíntesis",
          name: "Fotosíntesis",
          createdAt: iso(-7200000),
          updatedAt: iso(-7200000),
          materials: [],
          creations: [],
          publication: { state: "local", publicUrl: null },
          messages: [],
        },
      ];
    }

    persist();
  }

  seed("first-launch");

  // Callback plumbing (mimics Tauri IPC) ------------------------------------
  const callbacks = new Map();
  let cbCounter = 0;
  function transformCallback(cb, once = false) {
    const id = "cb_" + ++cbCounter;
    callbacks.set(id, { cb, once });
    return id;
  }
  function unregisterCallback(id) {
    callbacks.delete(id);
  }
  const listeners = new Map(); // event -> Map(eventId -> cbId)
  let evCounter = 0;
  function emitEvent(event, payload) {
    const map = listeners.get(event);
    if (!map) return;
    for (const [eventId, cbId] of map) {
      const entry = callbacks.get(cbId);
      if (entry) entry.cb({ event, id: eventId, payload });
    }
  }

  // Command router ----------------------------------------------------------
  const delay = (ms) => new Promise((r) => setTimeout(r, ms));
  const find = (id) => projects.find((x) => x.id === id);

  async function invoke(cmd, args = {}) {
    switch (cmd) {
      case "plugin:event|listen": {
        const ev = args.event;
        const cbId = args.handler;
        if (!listeners.has(ev)) listeners.set(ev, new Map());
        const eventId = "ev_" + ++evCounter;
        listeners.get(ev).set(eventId, cbId);
        return eventId;
      }
      case "plugin:event|unlisten":
        listeners.get(args.event)?.delete(args.eventId);
        return undefined;
      case "plugin:event|emit":
        emitEvent(args.event, args.payload);
        return undefined;
      case "plugin:dialog|open":
      case "plugin:dialog|save":
        return null;
      case "app_status":
        return { version: "0.1.0", agent: "opencode" };
      case "project_list":
        return projects
          .slice()
          .sort((a, b) => {
            const ta = new Date(a.updatedAt).getTime();
            const tb = new Date(b.updatedAt).getTime();
            if (tb !== ta) return tb - ta;
            return a.id.localeCompare(b.id);
          })
          .map(makeSummary);
      case "project_create": {
        const p = {
          id: makeId("proj"),
          name: args.name,
          createdAt: iso(0),
          updatedAt: iso(0),
          materials: [],
          creations: [],
          publication: { state: "local", publicUrl: null },
          messages: [],
        };
        projects.unshift(p);
        persist();
        return makeSummary(p);
      }
      case "project_open": {
        const p = find(args.projectId);
        if (!p) return Promise.reject({ code: "internal", message: "Proyecto no encontrado" });
        return makeProjectView(p);
      }
      case "project_rename": {
        const p = find(args.projectId);
        if (!p) return Promise.reject({ code: "internal", message: "Proyecto no encontrado" });
        p.name = args.name;
        p.updatedAt = iso(0);
        persist();
        return makeSummary(p);
      }
      case "project_delete":
        projects = projects.filter((x) => x.id !== args.projectId);
        persist();
        return undefined;
      case "material_add_from_path": {
        const p = find(args.projectId);
        const name = String(args.path).split("/").pop();
        const m = {
          id: makeId("mat"),
          displayName: name,
          originalFileName: name,
          kind: "file",
          byteSize: 4096,
          createdAt: iso(0),
        };
        p?.materials.push(m);
        persist();
        return m;
      }
      case "material_add_image": {
        const p = find(args.projectId);
        const m = {
          id: makeId("mat"),
          displayName: args.fileName,
          originalFileName: args.fileName,
          kind: "image",
          byteSize: (args.data || []).length,
          createdAt: iso(0),
        };
        p?.materials.push(m);
        persist();
        return { material: m, duplicate: false };
      }
      case "materials_add_from_paths": {
        const p = find(args.projectId);
        const items = (args.paths || []).map((path) => {
          const name = String(path).split("/").pop();
          const m = {
            id: makeId("mat"),
            displayName: name,
            originalFileName: name,
            kind: "file",
            byteSize: 4096,
            createdAt: iso(0),
          };
          p?.materials.push(m);
          return { sourceName: name, status: "added", materialId: m.id, material: m };
        });
        persist();
        return { items };
      }
      case "material_remove": {
        const p = find(args.projectId);
        if (p) {
          p.materials = p.materials.filter((m) => m.id !== args.materialId);
          persist();
        }
        return undefined;
      }
      case "material_open":
        return undefined;
      case "preview_data":
        return {
          contentType: "text/plain",
          dataBase64: btoa("Vista previa de ejemplo generada por el mock."),
        };
      case "preview_open_web":
      case "preview_close":
        return undefined;
      case "creation_set_visibility": {
        const p = find(args.projectId);
        const c = p?.creations.find((x) => x.id === args.creationId);
        if (c) c.visibility = args.public ? "public" : "private";
        persist();
        return c;
      }
      case "creation_open":
      case "open_public_url":
        return undefined;
      case "agent_send": {
        const p = find(args.projectId);
        if (!p) return undefined;
        const userMsg = {
          id: makeId("msg"),
          role: "user",
          text: String(args.prompt || ""),
          status: "ok",
          createdAt: new Date().toISOString(),
          materialIds: args.attachmentIds || [],
          creationIds: [],
        };
        p.messages.push(userMsg);
        p.updatedAt = userMsg.createdAt;
        persist();

        setTimeout(
          () =>
            emitEvent("agent://task", {
              projectId: args.projectId,
              status: "working",
              message: null,
              registeredCreationIds: [],
            }),
          50,
        );

        const assistantMsgId = makeId("msg");
        setTimeout(() => {
          const c = {
            id: makeId("cre"),
            displayName: "Actividad interactiva: " + String(args.prompt || "recurso").slice(0, 24),
            kind: "web",
            visibility: "public",
            byteSize: 512000,
            createdAt: new Date().toISOString(),
            revision: 1,
          };
          p.creations.push(c);
          const assistantMsg = {
            id: assistantMsgId,
            role: "assistant",
            text: "Listo.",
            status: "ok",
            createdAt: new Date().toISOString(),
            materialIds: [],
            creationIds: [c.id],
          };
          p.messages.push(assistantMsg);
          p.updatedAt = assistantMsg.createdAt;
          persist();
          emitEvent("agent://task", {
            projectId: args.projectId,
            status: "completed",
            message: "Listo.",
            registeredCreationIds: [c.id],
          });
        }, 900);
        return undefined;
      }
      case "agent_cancel": {
        const p = find(args.projectId);
        if (p) {
          const cancelledMsg = {
            id: makeId("msg"),
            role: "assistant",
            text: "Creación cancelada.",
            status: "cancelled",
            createdAt: new Date().toISOString(),
            materialIds: [],
            creationIds: [],
          };
          p.messages.push(cancelledMsg);
          p.updatedAt = cancelledMsg.createdAt;
          persist();
        }
        emitEvent("agent://task", {
          projectId: args.projectId,
          status: "cancelled",
          message: "Creación cancelada.",
          registeredCreationIds: [],
        });
        return undefined;
      }
      case "publish": {
        const p = find(args.projectId);
        if (p) {
          p.publication = {
            state: "published",
            publicUrl:
              "https://educai-demo-7k2x.trycloudflare.com/" +
              String(p.name).toLowerCase().replace(/[^a-z0-9]+/g, "-") +
              "-a7k2",
          };
          p.updatedAt = iso(0);
          persist();
        }
        await delay(1200);
        return p ? { ...p.publication } : { state: "local", publicUrl: null };
      }
      case "unpublish": {
        const p = find(args.projectId);
        if (p) {
          p.publication = { state: "local", publicUrl: null };
          p.updatedAt = iso(0);
          persist();
        }
        return p ? { ...p.publication } : { state: "local", publicUrl: null };
      }
      case "publication_status": {
        const p = find(args.projectId);
        return p ? { ...p.publication } : { state: "local", publicUrl: null };
      }
      case "model_list":
        return modelList;
      case "model_select": {
        const m = modelList.find((x) => x.providerId === args.providerId && x.modelId === args.modelId);
        selectedModel = { model: m || FREE_MODEL, notice: null, requiresChoice: false };
        persist();
        return undefined;
      }
      case "model_get_selected":
        return selectedModel;
      case "provider_list":
        return providerList;
      case "provider_detail":
        return { id: args.providerId, name: "Gratis", authMethods: PROVIDER_GRATIS.authMethods, connections: [] };
      case "provider_connect_key":
        return { id: makeId("conn"), label: args.label || "Conectado" };
      case "provider_disconnect":
        return undefined;
      case "provider_test_connection":
        return { outcome: "connected", message: "Conexion correcta." };
      case "provider_oauth_begin":
        return { attemptId: makeId("oauth"), url: "https://example.com/oauth", instructions: null, mode: "auto" };
      case "provider_oauth_status":
        return { status: "pending", message: null };
      case "provider_oauth_complete":
        return { id: makeId("conn"), label: "Cuenta conectada" };
      case "provider_oauth_cancel":
      case "provider_oauth_open":
        return undefined;
      default:
        return undefined;
    }
  }

  window.__TAURI_INTERNALS__ = {
    invoke,
    transformCallback,
    unregisterCallback,
    convertFileSrc: (p) => p,
    metadata: { currentWebview: { label: "main" }, currentWindow: { label: "main" } },
  };
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: () => {},
    registerListener: () => {},
  };
  window.__MOCK__ = {
    seed,
    emitEvent,
    getProjects: () => projects,
    clearPersisted,
    getSeed: () => seedName,
  };
  if (window.__UX_SEED__) seed(window.__UX_SEED__);
})();
