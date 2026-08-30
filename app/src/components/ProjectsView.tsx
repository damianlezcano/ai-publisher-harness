import { useState } from "react";
import { api, errorMessage } from "../api";
import type { ProjectSummary } from "../types";
import ConfirmDialog from "./ConfirmDialog";

interface ProjectsViewProps {
  projects: ProjectSummary[];
  onRefresh: () => Promise<void>;
  onOpen: (id: string) => void;
}

export default function ProjectsView({ projects, onRefresh, onOpen }: ProjectsViewProps) {
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingName, setEditingName] = useState("");
  const [deleting, setDeleting] = useState<ProjectSummary | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function create() {
    setBusy(true);
    setError(null);
    try {
      const created = await api.projectCreate(name);
      setCreating(false);
      setName("");
      await onRefresh();
      onOpen(created.id);
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function rename(id: string) {
    setBusy(true);
    setError(null);
    try {
      await api.projectRename(id, editingName);
      setEditingId(null);
      await onRefresh();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  async function remove() {
    if (!deleting) return;
    setBusy(true);
    setError(null);
    try {
      await api.projectDelete(deleting.id);
      setDeleting(null);
      await onRefresh();
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="view">
      <header className="view-header">
        <h1>Tus proyectos</h1>
        <button type="button" className="primary" onClick={() => setCreating((v) => !v)}>
          Nuevo proyecto
        </button>
      </header>

      {creating && (
        <form
          className="inline-form"
          onSubmit={(e) => {
            e.preventDefault();
            void create();
          }}
        >
          <label className="sr-only" htmlFor="new-project-name">
            Nombre del proyecto
          </label>
          <input
            id="new-project-name"
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Nombre del proyecto"
            autoFocus
          />
          <button type="submit" className="primary" disabled={busy || name.trim() === ""}>
            Crear
          </button>
          <button type="button" className="secondary" onClick={() => setCreating(false)}>
            Cancelar
          </button>
        </form>
      )}

      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}

      {projects.length === 0 && !creating ? (
        <p className="muted">Todavía no tenés proyectos. Creá el primero para empezar.</p>
      ) : (
        <ul className="project-list" aria-label="Proyectos">
          {projects.map((project) => (
            <li key={project.id} className="project-row">
              {editingId === project.id ? (
                <form
                  className="inline-form"
                  onSubmit={(e) => {
                    e.preventDefault();
                    void rename(project.id);
                  }}
                >
                  <label className="sr-only" htmlFor={`rename-${project.id}`}>
                    Nuevo nombre
                  </label>
                  <input
                    id={`rename-${project.id}`}
                    value={editingName}
                    onChange={(e) => setEditingName(e.target.value)}
                    autoFocus
                  />
                  <button
                    type="submit"
                    className="primary"
                    disabled={busy || editingName.trim() === ""}
                  >
                    Guardar
                  </button>
                  <button type="button" className="secondary" onClick={() => setEditingId(null)}>
                    Cancelar
                  </button>
                </form>
              ) : (
                <>
                  <span className="project-name">{project.name}</span>
                  <span className="row-actions">
                    <button type="button" className="primary" onClick={() => onOpen(project.id)}>
                      Abrir
                    </button>
                    <button
                      type="button"
                      className="secondary"
                      onClick={() => {
                        setEditingId(project.id);
                        setEditingName(project.name);
                      }}
                    >
                      Renombrar
                    </button>
                    <button type="button" className="danger" onClick={() => setDeleting(project)}>
                      Eliminar
                    </button>
                  </span>
                </>
              )}
            </li>
          ))}
        </ul>
      )}

      {deleting && (
        <ConfirmDialog
          title="Eliminar proyecto"
          message={`Escribí “${deleting.name}” para confirmar la eliminación.`}
          confirmText={deleting.name}
          busy={busy}
          onCancel={() => setDeleting(null)}
          onConfirm={() => void remove()}
        />
      )}
    </div>
  );
}
