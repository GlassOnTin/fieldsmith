// Single bidirectional Human <-> AI workspace synchronizer (`/api/workspace`).
//
// Invariants:
// 1. Exactly one poller runs per browser tab (replacing duplicate loops in `systems.tsx`).
// 2. Incoming MCP/external `.fsm` updates apply via `store.applyExternalDoc(...)` so the
//    human's Undo/Redo stack (`store.past`), active selection, and 3D camera are preserved.
// 3. Outgoing human `store.transact` mutations debounce-push `doc_fsm` with `expected_revision`,
//    surfacing HTTP `409 Conflict` explicitly instead of silently clobbering concurrent AI edits.

import { store } from './store';
import { parseFsm, printFsm } from './wasm';

export interface WorkspaceActivityEntry {
  revision: number;
  timestamp_ms: number;
  actor: string;
  tool: string;
  summary: string;
}

export interface WorkspaceSnapshot {
  revision: number;
  updated_at_ms: number;
  actor: string;
  last_tool: string;
  doc_fsm?: string | null;
  coupled_system_id?: string | null;
  design_p?: number[] | null;
  epistemic_theta?: number[] | null;
  remote_compute_url?: string | null;
  activity_log?: WorkspaceActivityEntry[];
}

export type WorkspaceSyncStatus = 'offline' | 'synced' | 'pushing' | 'conflict';

interface WorkspaceSyncOptions {
  resolveUrl: (path: string) => string;
  onNotify?: (message: string, kind?: 'info' | 'error') => void;
}

type SnapshotListener = (ws: WorkspaceSnapshot, source: 'poll' | 'push' | 'conflict') => void;
type StatusListener = () => void;

let lastKnownRevision = -1;
let serverAvailable = false;
let syncStatus: WorkspaceSyncStatus = 'offline';
let lastActor = 'ready';
let conflictRemote: WorkspaceSnapshot | null = null;
let started = false;
let debounceTimer = 0;
let pollTimer = 0;
let resolveUrlFn: (path: string) => string = (p) => p;
let notifyFn: ((msg: string, kind?: 'info' | 'error') => void) | undefined;

const snapshotListeners = new Set<SnapshotListener>();
const statusListeners = new Set<StatusListener>();

function emitStatus() {
  for (const l of statusListeners) l();
}

function emitSnapshot(ws: WorkspaceSnapshot, source: 'poll' | 'push' | 'conflict') {
  for (const l of snapshotListeners) l(ws, source);
}

export function getWorkspaceSyncState() {
  return {
    revision: lastKnownRevision,
    serverAvailable,
    status: syncStatus,
    lastActor,
    conflictRemote,
  };
}

export function subscribeWorkspaceSync(listener: StatusListener): () => void {
  statusListeners.add(listener);
  return () => statusListeners.delete(listener);
}

export function subscribeWorkspaceSnapshots(listener: SnapshotListener): () => void {
  snapshotListeners.add(listener);
  return () => snapshotListeners.delete(listener);
}

export function markWorkspaceRevisionSeen(rev: number): void {
  if (typeof rev === 'number' && Number.isFinite(rev)) {
    lastKnownRevision = Math.max(lastKnownRevision, rev);
    serverAvailable = true;
    if (syncStatus === 'offline') syncStatus = 'synced';
    emitStatus();
  }
}

export async function pushWorkspaceSnapshot(
  payload: {
    actor?: string;
    last_tool?: string;
    summary?: string;
    doc_fsm?: string;
    coupled_system_id?: string;
    design_p?: number[];
    epistemic_theta?: number[];
  },
  opts?: { forceOverwrite?: boolean },
): Promise<WorkspaceSnapshot | null> {
  try {
    const body: Record<string, unknown> = {
      actor: 'web-ui',
      ...payload,
    };
    if (!opts?.forceOverwrite && lastKnownRevision >= 0) {
      body.expected_revision = lastKnownRevision;
    }
    syncStatus = 'pushing';
    emitStatus();
    const resp = await fetch(resolveUrlFn('/api/workspace'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (resp.status === 409) {
      const errBody = (await resp.json().catch(() => ({}))) as {
        current_revision?: number;
        workspace?: WorkspaceSnapshot;
      };
      conflictRemote = errBody.workspace ?? null;
      syncStatus = 'conflict';
      emitStatus();
      if (conflictRemote) emitSnapshot(conflictRemote, 'conflict');
      notifyFn?.(
        `Workspace conflict (remote rev #${errBody.current_revision ?? '?'} changed concurrently). Choose Accept Remote or Keep Local in the status bar.`,
        'error',
      );
      return null;
    }
    if (!resp.ok) {
      syncStatus = serverAvailable ? 'synced' : 'offline';
      emitStatus();
      return null;
    }
    const ws = (await resp.json()) as WorkspaceSnapshot;
    serverAvailable = true;
    conflictRemote = null;
    if (typeof ws.revision === 'number') {
      lastKnownRevision = Math.max(lastKnownRevision, ws.revision);
    }
    lastActor = `${ws.actor}:${ws.last_tool}`;
    syncStatus = 'synced';
    emitStatus();
    emitSnapshot(ws, 'push');
    return ws;
  } catch {
    syncStatus = 'offline';
    emitStatus();
    return null;
  }
}

export function resolveConflictAcceptRemote(): boolean {
  if (!conflictRemote) return false;
  const ws = conflictRemote;
  conflictRemote = null;
  if (typeof ws.revision === 'number') {
    lastKnownRevision = ws.revision;
  }
  syncStatus = 'synced';
  let applied = false;
  if (typeof ws.doc_fsm === 'string' && ws.doc_fsm.trim().length > 0) {
    const parsed = parseFsm(ws.doc_fsm);
    if ('ok' in parsed) {
      applied = store.applyExternalDoc(
        parsed.ok,
        `MCP rev #${ws.revision} (${ws.last_tool || 'sync'})`,
      );
    }
  }
  emitStatus();
  emitSnapshot(ws, 'poll');
  notifyFn?.(`Accepted remote workspace rev #${ws.revision} (Ctrl+Z undoes)`, 'info');
  return applied;
}

export async function resolveConflictKeepLocal(): Promise<boolean> {
  conflictRemote = null;
  const docFsm = printFsm(store.doc);
  const ws = await pushWorkspaceSnapshot(
    {
      actor: 'web-ui',
      last_tool: 'resolve-conflict-keep-local',
      summary: `Kept local '${store.doc.name}' over concurrent edit`,
      doc_fsm: docFsm,
      coupled_system_id: store.doc.coupled_system ?? undefined,
    },
    { forceOverwrite: true },
  );
  if (ws) {
    notifyFn?.(`Published local document as workspace rev #${ws.revision}`, 'info');
    return true;
  }
  return false;
}

async function pollWorkspaceOnce(): Promise<void> {
  if (syncStatus === 'conflict') return;
  try {
    const resp = await fetch(resolveUrlFn('/api/workspace'));
    if (!resp.ok) return;
    const ws = (await resp.json()) as WorkspaceSnapshot;
    serverAvailable = true;
    const rev = typeof ws.revision === 'number' ? ws.revision : 0;

    if (lastKnownRevision < 0) {
      lastKnownRevision = rev;
      lastActor = `${ws.actor || 'ready'}:${ws.last_tool || 'init'}`;
      syncStatus = 'synced';
      // Only adopt pre-existing disk workspace on startup if explicitly requested (?sync=1),
      // so opening a fresh browser tab starts cleanly with EmptyState / localStorage restore.
      const autoAdopt =
        typeof window !== 'undefined' && new URLSearchParams(window.location.search).has('sync');
      if (
        autoAdopt &&
        store.doc.root.length === 0 &&
        store.doc.name === 'Untitled' &&
        typeof ws.doc_fsm === 'string' &&
        ws.doc_fsm.trim().length > 0
      ) {
        const parsed = parseFsm(ws.doc_fsm);
        if ('ok' in parsed) {
          store.load(parsed.ok, { fromExternalSync: true });
        }
      }
      emitStatus();
      emitSnapshot(ws, 'poll');
      return;
    }

    if (rev > lastKnownRevision) {
      lastKnownRevision = rev;
      lastActor = `${ws.actor}:${ws.last_tool}`;
      syncStatus = 'synced';
      if (ws.actor !== 'web-ui' && typeof ws.doc_fsm === 'string' && ws.doc_fsm.trim().length > 0) {
        const parsed = parseFsm(ws.doc_fsm);
        if ('ok' in parsed) {
          const changed = store.applyExternalDoc(
            parsed.ok,
            `MCP #${ws.revision}: ${ws.last_tool || 'update'}`,
          );
          if (changed) {
            notifyFn?.(
              `Synced MCP update rev #${ws.revision} (${ws.last_tool}) — undoable with Ctrl+Z`,
              'info',
            );
          }
        } else {
          notifyFn?.(
            `Workspace .fsm parse error (line ${parsed.error.line}): ${parsed.error.message}`,
            'error',
          );
        }
      }
      emitStatus();
      emitSnapshot(ws, 'poll');
    }
  } catch {
    if (!conflictRemote) {
      syncStatus = 'offline';
      emitStatus();
    }
  }
}

export function startWorkspaceSync(opts: WorkspaceSyncOptions): () => void {
  resolveUrlFn = opts.resolveUrl;
  notifyFn = opts.onNotify;
  if (started) {
    return () => {};
  }
  started = true;

  const unsubLocal = store.subscribeLocalMutations((doc, label) => {
    if (!serverAvailable || syncStatus === 'conflict') return;
    window.clearTimeout(debounceTimer);
    debounceTimer = window.setTimeout(() => {
      try {
        const docFsm = printFsm(doc);
        void pushWorkspaceSnapshot({
          actor: 'web-ui',
          last_tool: 'store.transact',
          summary: label,
          doc_fsm: docFsm,
          coupled_system_id: doc.coupled_system ?? undefined,
        });
      } catch {
        // WASM not ready or serialization skipped
      }
    }, 300);
  });

  void pollWorkspaceOnce();
  pollTimer = window.setInterval(() => {
    void pollWorkspaceOnce();
  }, 1200);

  return () => {
    started = false;
    unsubLocal();
    window.clearTimeout(debounceTimer);
    window.clearInterval(pollTimer);
  };
}
