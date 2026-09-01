import { useState } from "react";
import { api } from "../api";
import type { PublicationView } from "../types";

export interface UseShareControlInput {
  projectId: string;
  publication: PublicationView;
  onRefresh: () => void | Promise<void>;
}

export type Busy = "publishing" | "unpublishing" | null;

export interface ShareControlState {
  busy: Busy;
  error: unknown | null;
  copyFailed: boolean;
  copied: boolean;
  showQr: boolean;
  setShowQr: (value: boolean) => void;
  confirmStop: boolean;
  setConfirmStop: (value: boolean) => void;
  menuOpen: boolean;
  setMenuOpen: (value: boolean | ((prev: boolean) => boolean)) => void;
  shared: boolean;
  publish: () => Promise<void>;
  unpublish: () => Promise<void>;
  copy: () => Promise<void>;
  open: () => Promise<void>;
}

export function useShareControl({
  projectId,
  publication,
  onRefresh,
}: UseShareControlInput): ShareControlState {
  const [busy, setBusy] = useState<Busy>(null);
  const [error, setError] = useState<unknown | null>(null);
  const [copyFailed, setCopyFailed] = useState(false);
  const [copied, setCopied] = useState(false);
  const [showQr, setShowQr] = useState(false);
  const [confirmStop, setConfirmStop] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);

  const shared = publication.state === "published" && publication.publicUrl !== null;

  async function publish() {
    setBusy("publishing");
    setError(null);
    try {
      await api.publish(projectId);
      await onRefresh();
    } catch (err) {
      setError(err);
    } finally {
      setBusy(null);
    }
  }

  async function unpublish() {
    setConfirmStop(false);
    setBusy("unpublishing");
    setError(null);
    try {
      await api.unpublish(projectId);
      await onRefresh();
    } catch (err) {
      setError(err);
    } finally {
      setBusy(null);
    }
  }

  async function copy() {
    if (!publication.publicUrl) return;
    setCopyFailed(false);
    try {
      await navigator.clipboard.writeText(publication.publicUrl);
      setCopied(true);
    } catch {
      setCopyFailed(true);
    }
  }

  async function open() {
    setError(null);
    try {
      await api.openPublicUrl(projectId);
    } catch (err) {
      setError(err);
    }
  }

  return {
    busy,
    error,
    copyFailed,
    copied,
    showQr,
    setShowQr,
    confirmStop,
    setConfirmStop,
    menuOpen,
    setMenuOpen,
    shared,
    publish,
    unpublish,
    copy,
    open,
  };
}
