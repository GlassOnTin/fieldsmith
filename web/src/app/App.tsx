// The application shell: tree | viewport | inspector, status line below.
import type { JSX } from 'preact';
import { useEffect, useRef, useState } from 'preact/hooks';
import { store, LENSES } from '../doc/store';
import {
  getWorkspaceSyncState,
  resolveConflictAcceptRemote,
  resolveConflictKeepLocal,
  subscribeWorkspaceSync,
} from '../doc/workspace-sync';
import { FACE_LABEL } from '../doc/types';
import type { Face } from '../doc/types';
import { parseFsm, printFsm } from '../doc/wasm';
import { Tree } from './Tree';
import { Inspector } from './Inspector';
import { ViewportHost } from './ViewportHost';
import { Palette, Menus } from './Palette';
import { EXAMPLES, addShape, importMeshCommand, newDocument, openFile, openText, runCommand, saveFile } from './commands';
import {
  openAntennaWorkbenchModal,
  openCoordMappingModal,
  openFixingsCatalogueModal,
  openFreeSurfaceModal,
  openSketchWorkbenchModal,
  toggleTutorialsDrawer,
} from './modal-launchers';
import { extractDroneAntennaSpecFromDoc } from '../doc/antenna-vna';
import { matchedCoupledSystemIdForDoc } from '../doc/continuum';
import { Icon } from './icons';
import { ui, useApp } from './ui';

function LazyMount({
  active,
  load,
}: {
  active: boolean;
  load: () => Promise<() => JSX.Element | null>;
}) {
  const [Comp, setComp] = useState<(() => JSX.Element | null) | null>(null);
  const started = useRef(false);
  useEffect(() => {
    if (!active || started.current) return;
    started.current = true;
    void load().then((fn) => setComp(() => fn));
  }, [active]);
  return Comp ? <Comp /> : null;
}

function WorkspaceSyncPill() {
  const [, setTick] = useState(0);
  useEffect(() => subscribeWorkspaceSync(() => setTick((t) => t + 1)), []);
  const sync = getWorkspaceSyncState();
  if (sync.status !== 'conflict' || !sync.conflictRemote) return null;
  return (
    <>
      <span class="sep">|</span>
      <span
        data-testid="workspace-conflict-banner"
        style={{
          display: 'inline-flex',
          alignItems: 'center',
          gap: '4px',
          padding: '1px 6px',
          borderRadius: '4px',
          background: 'rgba(239, 68, 68, 0.16)',
          border: '1px solid var(--danger)',
          color: '#fca5a5',
          fontSize: '10.5px',
          fontFamily: 'var(--mono)',
        }}
        title={`Workspace conflict: remote rev #${sync.conflictRemote.revision} (${sync.conflictRemote.actor}:${sync.conflictRemote.last_tool}) modified the workspace while you had local edits`}
      >
        <span>⚠ Conflict rev #{sync.conflictRemote.revision}</span>
        <button
          type="button"
          class="btn small"
          data-testid="workspace-conflict-accept-remote"
          onClick={() => resolveConflictAcceptRemote()}
          style={{ height: '16px', padding: '0 5px', fontSize: '10px' }}
        >
          Accept Remote
        </button>
        <button
          type="button"
          class="btn small"
          data-testid="workspace-conflict-keep-local"
          onClick={() => void resolveConflictKeepLocal()}
          style={{ height: '16px', padding: '0 5px', fontSize: '10px' }}
        >
          Keep Local
        </button>
      </span>
    </>
  );
}

/** Lens-specific viewport overlays and status items (registered by later lenses). */
export const viewOverlays: (() => JSX.Element | null)[] = [];
export const statusItems: (() => JSX.Element | null)[] = [];

function Lenses() {
  return (
    <div class="lenses" role="tablist" aria-label="Lens">
      {LENSES.map((l) => (
        <button
          key={l.id}
          role="tab"
          class={store.lens === l.id ? 'on' : ''}
          aria-selected={store.lens === l.id}
          data-lens={l.id}
          title={
            l.id === 'theory' && store.lens === 'theory'
              ? `Close Theory & return to ${store.previousNonTheoryLens} (Esc)`
              : `${l.label} (${l.key})`
          }
          onClick={() => {
            if (ui.tutorialsOpen && !ui.tutorialMinimised) {
              ui.set('tutorialMinimised', true);
            }
            store.setLens(l.id === 'theory' && store.lens === 'theory' ? store.previousNonTheoryLens : l.id);
          }}
        >
          {l.label}
          <span class="kbd">{l.key}</span>
        </button>
      ))}
    </div>
  );
}

function ViewTools() {
  const isMobile = typeof window !== 'undefined' && window.innerWidth <= 900;
  const clearMode = isMobile ? ui.vpHudCollapsed : ui.treeCollapsed && ui.inspCollapsed;
  const showExpandInsp = isMobile ? !ui.inspOpen : ui.inspCollapsed;
  return (
    <div class="view-tools">
      {showExpandInsp && (
        <button
          class="btn icon"
          data-testid="expand-insp-btn"
          title="Open Inspector sidebar (])"
          onClick={() => runCommand('view.toggleInsp')}
        >
          <Icon.sidebarRight />
        </button>
      )}
      {store.lens !== 'theory' && (
        <button class="btn icon" title="Fit model (F)" onClick={() => runCommand('view.fit')}>
          <Icon.fit />
        </button>
      )}
      <button
        class={`btn icon ${clearMode ? 'on' : ''}`}
        data-testid="toggle-clear-viewport"
        title={clearMode ? 'Restore sidebars & HUD overlays (\\)' : 'Clear 3D viewport / Toggle sidebars (\\)'}
        onClick={() => runCommand('view.toggleSidebars')}
      >
        <Icon.sidebarsBoth />
      </button>
      <button
        class={`btn icon ${ui.fsmDrawerOpen ? 'on' : ''}`}
        data-testid="hud-open-fsm-drawer"
        title="Live .fsm source editor (Ctrl E)"
        onClick={() => runCommand('file.editFsmSource')}
      >
        <span style={{ fontWeight: 700, fontSize: '11px', fontFamily: 'var(--mono)' }}>{'{}'}</span>
      </button>
      <button
        class={`btn icon ${ui.tutorialsOpen ? 'on' : ''}`}
        data-testid="open-tutorials-drawer"
        title="Tutorials, Analytic Verification Tests (A1–A25) & Case Studies (T1–T28)"
        onClick={() => runCommand('tutorials.open')}
      >
        <span style={{ fontWeight: 700, fontSize: '13px', fontFamily: 'var(--mono)' }}>?</span>
      </button>
      {store.lens === 'shape' && (
        <>
          <button
            class="btn icon"
            data-testid="hud-open-sketch-workbench"
            title="Parametric 2D Sketcher & Surface-Feature Engine (LM + QR Solver)"
            onClick={() => openSketchWorkbenchModal()}
          >
            <Icon.sketcher />
          </button>
          <button
            class="btn icon"
            data-testid="hud-open-fixings-catalog"
            title="Engineering Fixings & Snap-Fit Catalogue (Auto-Tap Host)"
            onClick={() => openFixingsCatalogueModal()}
          >
            <Icon.fixing />
          </button>
          {extractDroneAntennaSpecFromDoc(store.doc) && (
            <button
              class="btn icon"
              data-testid="hud-open-antenna-workbench"
              title="RF Antenna VNA / Smith Chart / 3D Lobe Workbench"
              onClick={() => openAntennaWorkbenchModal()}
            >
              <Icon.antenna />
            </button>
          )}
          {matchedCoupledSystemIdForDoc(store.doc, ui.tutorialSheetId) && (
            <button
              class={`btn icon ${ui.systemsOpen ? 'on' : ''}`}
              data-testid="hud-open-systems-workbench"
              title="Coupled 1D/2D System Solver for this Model"
              onClick={() => runCommand('systems.open')}
            >
              <span style={{ fontWeight: 700, fontSize: '10px', fontFamily: 'var(--mono)' }}>PDE</span>
            </button>
          )}
          <button class={`btn icon ${ui.gizmo === 'translate' ? 'on' : ''}`} title="Move (W)" onClick={() => runCommand('tool.move')}>
            <Icon.move />
          </button>
          <button class={`btn icon ${ui.gizmo === 'rotate' ? 'on' : ''}`} title="Rotate (E)" onClick={() => runCommand('tool.rotate')}>
            <Icon.rotate />
          </button>
          <button class={`btn icon ${ui.snap ? 'on' : ''}`} title={`Snap to ${ui.gridStep} mm grid and shape centres (S)`} onClick={() => runCommand('tool.snap')}>
            <Icon.magnet />
          </button>
        </>
      )}
    </div>
  );
}

function EmptyState() {
  if (store.doc.root.length > 0 || store.lens !== 'shape') return null;
  return (
    <div class="empty-state">
      <div class="empty-card" data-testid="empty-state">
        <h2>Start with a shape</h2>
        <p>Parts are built from simple shapes that add, cut or intersect, in order. Everything stays editable.</p>
        <div class="actions">
          <button class="btn primary" onClick={() => addShape('box')} data-testid="empty-add-box">
            <Icon.box /> Add a box
          </button>
          <button class="btn" onClick={() => addShape('cylinder')}>
            <Icon.cylinder /> Cylinder
          </button>
          <button class="btn" onClick={importMeshCommand} data-testid="empty-import-mesh">
            <Icon.mesh /> Import mesh…
          </button>
          <button class="btn" onClick={openFile}>
            Open .fsm…
          </button>
        </div>

        {/* Compact 1-line Starter Models & Collapsed Contents Index */}
        <div
          style={{
            marginTop: '12px',
            paddingTop: '10px',
            borderTop: '1px solid var(--line)',
            display: 'flex',
            flexDirection: 'column',
            gap: '8px',
          }}
        >
          <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: '5px' }}>
            <span style={{ fontSize: '11px', color: 'var(--text-3)', marginRight: '2px' }}>Starter:</span>
            {EXAMPLES.map((e) => (
              <button key={e.id} class="btn small" onClick={() => openText(e.text, `${e.id}.fsm`)} data-testid={`example-${e.id}`}>
                {e.label}
              </button>
            ))}
          </div>

          <details
            data-testid="empty-suites-contents"
            style={{
              background: 'var(--panel-2)',
              border: '1px solid var(--line)',
              borderRadius: '6px',
              padding: '6px 9px',
              fontSize: '11.5px',
            }}
          >
            <summary
              data-testid="empty-suites-summary"
              style={{
                cursor: 'pointer',
                userSelect: 'none',
                display: 'flex',
                flexWrap: 'wrap',
                alignItems: 'center',
                justifyContent: 'space-between',
                gap: '6px',
                fontWeight: 600,
                color: 'var(--text-2)',
              }}
            >
              <span>📚 Contents: Analytic Tests &amp; Tutorials</span>
              <span
                style={{ display: 'inline-flex', flexWrap: 'wrap', gap: '4px' }}
                onClick={(e) => e.stopPropagation()}
              >
                <button
                  type="button"
                  class="btn small primary"
                  onClick={() => {
                    ui.set('tutorialMinimised', false);
                    ui.set('tutorialCategoryFilter', 'analytic');
                    ui.set('tutorialContentsOpen', true);
                    ui.set('tutorialsOpen', true);
                  }}
                  data-testid="empty-open-analytic-tests"
                  style={{
                    background: '#0891b2',
                    borderColor: '#06b6d4',
                    color: '#fff',
                    padding: '1px 7px',
                    fontSize: '10.5px',
                  }}
                >
                  📐 Analytic (20)
                </button>
                <button
                  type="button"
                  class="btn small primary"
                  onClick={() => {
                    ui.set('tutorialMinimised', false);
                    ui.set('tutorialCategoryFilter', 'tutorial');
                    ui.set('tutorialContentsOpen', true);
                    ui.set('tutorialsOpen', true);
                  }}
                  data-testid="empty-open-tutorials"
                  style={{ padding: '1px 7px', fontSize: '10.5px' }}
                >
                  📘 Tutorials (10)
                </button>
                <button
                  type="button"
                  class="btn small"
                  onClick={() => {
                    ui.set('tutorialMinimised', false);
                    ui.set('tutorialCategoryFilter', 'case-study');
                    ui.set('tutorialContentsOpen', true);
                    ui.set('tutorialsOpen', true);
                  }}
                  data-testid="empty-open-case-studies"
                  style={{ padding: '1px 7px', fontSize: '10.5px' }}
                >
                  🚀 Cases (9)
                </button>
              </span>
            </summary>
            <div
              style={{
                marginTop: '7px',
                paddingTop: '6px',
                borderTop: '1px solid var(--line)',
                display: 'flex',
                flexDirection: 'column',
                gap: '4px',
                fontSize: '11px',
                color: 'var(--text-2)',
              }}
            >
              <div style={{ display: 'flex', justifyContent: 'space-between', gap: '8px' }}>
                <span><strong>Track 0 (A1–A25)</strong>: Elasticity, Thermal, Gerris &amp; Capillary Benchmarks</span>
                <button
                  type="button"
                  class="btn ghost small"
                  style={{ padding: '0 6px', fontSize: '10.5px', color: '#22d3ee' }}
                  onClick={() => {
                    ui.set('tutorialMinimised', false);
                    ui.set('tutorialCategoryFilter', 'analytic');
                    ui.set('tutorialContentsOpen', true);
                    ui.set('tutorialsOpen', true);
                  }}
                >
                  Index →
                </button>
              </div>
              <div style={{ display: 'flex', justifyContent: 'space-between', gap: '8px' }}>
                <span><strong>Tracks I–II (T1–T7)</strong>: CSG Foundations, Thermal &amp; Modal FEA</span>
                <button
                  type="button"
                  class="btn ghost small"
                  style={{ padding: '0 6px', fontSize: '10.5px' }}
                  onClick={() => {
                    ui.set('tutorialMinimised', false);
                    ui.set('tutorialCategoryFilter', 'tutorial');
                    ui.set('tutorialContentsOpen', true);
                    ui.set('tutorialsOpen', true);
                  }}
                >
                  Index →
                </button>
              </div>
              <div style={{ display: 'flex', justifyContent: 'space-between', gap: '8px' }}>
                <span><strong>Tracks III–IV (T8–T19)</strong>: Multiphysics, Fasteners, RF VNA, SPH &amp; Clinching</span>
                <button
                  type="button"
                  class="btn ghost small"
                  style={{ padding: '0 6px', fontSize: '10.5px' }}
                  onClick={() => {
                    ui.set('tutorialMinimised', false);
                    ui.set('tutorialCategoryFilter', 'case-study');
                    ui.set('tutorialContentsOpen', true);
                    ui.set('tutorialsOpen', true);
                  }}
                >
                  Index →
                </button>
              </div>
            </div>
          </details>
        </div>
      </div>
    </div>
  );
}

function StatusBar() {
  const h = ui.hover;
  const s = store.selection;
  const fps = ui.frameMs > 0 ? Math.min(999, Math.round(1000 / Math.max(ui.frameMs, 1))) : 0;
  return (
    <div class="status" data-testid="status">
      <span class="grow">
        {h ? (
          <>
            <span>{h.shape ?? 'surface'}</span>
            {h.face && h.face !== 'all' && <span> · {FACE_LABEL[h.face as Face]}</span>}
            <span class="num" style={{ marginLeft: '10px', color: 'var(--text-3)' }}>
              {h.point.map((v) => v.toFixed(2)).join(', ')} mm
            </span>
          </>
        ) : store.lastAction ? (
          <span>{store.lastAction}</span>
        ) : (
          <span>Ctrl K for every command</span>
        )}
      </span>
      {statusItems.map((S, i) => (
        <S key={i} />
      ))}
      <WorkspaceSyncPill />
      <span class="sep">|</span>
      <button
        class="btn ghost small"
        data-testid="footer-free-surface-sph"
        style={{ height: '20px', padding: '0 5px', fontSize: '11px', color: '#38bdf8' }}
        title="Open 3D Multi-Regime Free-Surface & Capillary Fluid Workbench"
        onClick={() => openFreeSurfaceModal()}
      >
        🌊 SPH
      </button>
      <span class="sep">|</span>
      <button
        class="btn ghost small"
        data-testid="footer-coord-mapping"
        style={{ height: '20px', padding: '0 5px', fontSize: '11px', color: '#a855f7' }}
        title="Open 1D–4D Coordinate Mappings & Pullback FEM Suite"
        onClick={() => openCoordMappingModal()}
      >
        🌐 Coord
      </button>
      <span class="sep">|</span>
      <button
        class="btn ghost small"
        data-testid="footer-analytic-tests"
        style={{ height: '20px', padding: '0 5px', fontSize: '11px', color: '#22d3ee' }}
        title="Open Closed-Form Analytic Verification Benchmarks Contents (A1–A25)"
        onClick={() => {
          ui.set('tutorialMinimised', false);
          ui.set('tutorialCategoryFilter', 'analytic');
          ui.set('tutorialContentsOpen', true);
          ui.set('tutorialsOpen', true);
        }}
      >
        📐 A1–A25
      </button>
      <span class="sep">|</span>
      <button
        class="btn ghost small"
        data-testid="footer-tutorials"
        style={{ height: '20px', padding: '0 5px', fontSize: '11px' }}
        title="Open Guided Tutorials & Multiphysics Case Studies Contents (T1–T19)"
        onClick={toggleTutorialsDrawer}
      >
        📘 T1–T19
      </button>
      <span class="sep">|</span>
      <button
        class="btn ghost small"
        data-testid="footer-mcp-setup"
        style={{ height: '20px', padding: '0 5px', fontSize: '11px', color: 'var(--accent)' }}
        title="View MCP Server & REST API Co-Pilot Setup Instructions"
        onClick={() => ui.set('mcpSetupOpen', !ui.mcpSetupOpen)}
      >
        MCP
      </button>
      {s && (
        <>
          <span class="sep">|</span>
          <span title="Deselect with Esc, a right-click in the viewport, or clicking empty space">
            Selected: {s.id} (Esc)
          </span>
        </>
      )}
      <span class="sep">|</span>
      <button
        type="button"
        class="btn small"
        data-testid="status-compute-pill"
        title="Click to configure compute backend, scan LAN for Fieldsmith workstations, or connect MCP"
        onClick={() => ui.set('mcpSetupOpen', true)}
        style={{
          padding: '1px 6px',
          fontSize: '10.5px',
          height: '18px',
          lineHeight: '16px',
          fontFamily: 'var(--mono)',
          borderColor: ui.computeMode === 'remote' ? 'var(--accent)' : 'var(--line)',
          color: ui.computeMode === 'remote' ? 'var(--accent)' : 'var(--text-2)',
        }}
      >
        {ui.computeMode === 'remote'
          ? `⚡ Remote: ${ui.computeServerUrl.replace(/^https?:\/\//, '') || 'LAN'}`
          : ui.computeMode === 'native'
            ? '⚡ Native Host'
            : 'WASM'}
      </button>
      <span class="sep">|</span>
      <span class="num" title="Render time of the last frames">
        {ui.frameMs.toFixed(1)} ms{fps ? ` · ${fps} fps max` : ''}
      </span>
      <span class="sep">|</span>
      <span>mm · N · MPa</span>
    </div>
  );
}

/** Persistent banner over the viewport while a rendering failure leaves it blank or
 * wrong (loud rendering failures, VISION §7). Unlike the toast it stays for as long as
 * the broken state does — the next successful shader compile, a document whose field
 * has a surface again, or a results build that succeeds clears it. */
function RenderErrorSurface() {
  const e = ui.renderError;
  if (!e) return null;
  const label =
    e.kind === 'shader'
      ? 'The view shader failed'
      : e.kind === 'surface'
        ? 'Nothing to draw'
        : 'Results failed to build';
  return (
    <div class="render-error" role="alert" data-testid="render-error" data-kind={e.kind}>
      <div class="head">{label}</div>
      <pre>{e.text}</pre>
    </div>
  );
}

function FsmSourceDrawer() {
  useApp();
  const [draft, setDraft] = useState('');
  const [dirty, setDirty] = useState(false);
  const [parseErr, setParseErr] = useState<string | null>(null);

  useEffect(() => {
    if (!ui.fsmDrawerOpen) {
      setDirty(false);
      setParseErr(null);
      return;
    }
    if (!dirty) {
      try {
        setDraft(printFsm(store.doc));
        setParseErr(null);
      } catch (e) {
        setParseErr(e instanceof Error ? e.message : String(e));
      }
    }
  }, [ui.fsmDrawerOpen, store.docVersion, dirty]);

  if (!ui.fsmDrawerOpen) return null;

  const validateText = (txt: string) => {
    const r = parseFsm(txt);
    if ('error' in r) {
      setParseErr(`Line ${r.error.line}: ${r.error.message}`);
      return null;
    }
    setParseErr(null);
    return r.ok;
  };

  const applySource = () => {
    const ok = validateText(draft);
    if (!ok) {
      ui.notify(parseErr ?? 'Invalid .fsm syntax', 'error');
      return;
    }
    const changed = store.replaceDoc(ok, 'Edit .fsm source');
    setDirty(false);
    setDraft(printFsm(store.doc));
    ui.notify(changed ? 'Applied .fsm source (Ctrl+Z to undo)' : '.fsm source is already up to date');
  };

  return (
    <div
      class="modal-backdrop"
      data-testid="fsm-source-drawer"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) ui.set('fsmDrawerOpen', false);
      }}
    >
      <div
        class="modal-card"
        role="dialog"
        aria-label=".fsm Source Editor"
        style={{ width: 'min(780px, 95vw)', height: 'min(680px, 88vh)' }}
      >
        <div class="modal-head">
          <div style={{ display: 'flex', alignItems: 'center', gap: '8px', flexWrap: 'wrap' }}>
            <span class="sys-badge">{'{}'} .FSM</span>
            <strong style={{ fontSize: '13px' }}>Declarative .fsm Source ({store.doc.name})</strong>
            <span
              data-testid="fsm-source-status"
              class={parseErr ? 'bad-pill' : 'ok-pill'}
              style={{ fontWeight: 600 }}
            >
              {parseErr ? parseErr : dirty ? 'Valid · unapplied edits' : 'Synced'}
            </span>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: '6px' }}>
            <button
              type="button"
              class="btn small"
              data-testid="fsm-source-reset-btn"
              disabled={!dirty}
              onClick={() => {
                setDraft(printFsm(store.doc));
                setDirty(false);
                setParseErr(null);
              }}
            >
              Reset
            </button>
            <button
              type="button"
              class="btn small primary"
              data-testid="fsm-source-apply-btn"
              disabled={!!parseErr}
              title="Parse and apply .fsm source as an undoable edit (Ctrl+Enter)"
              onClick={applySource}
            >
              Apply .fsm (Ctrl+Enter)
            </button>
            <button
              type="button"
              class="btn small ghost"
              data-testid="fsm-source-close-btn"
              title="Close .fsm editor (Esc)"
              onClick={() => ui.set('fsmDrawerOpen', false)}
            >
              ✕
            </button>
          </div>
        </div>
        <textarea
          class="fsm-source"
          data-testid="fsm-source-textarea"
          value={draft}
          spellcheck={false}
          onInput={(e) => {
            const next = (e.target as HTMLTextAreaElement).value;
            setDraft(next);
            setDirty(true);
            validateText(next);
          }}
          onKeyDown={(e) => {
            e.stopPropagation();
            if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
              e.preventDefault();
              applySource();
            } else if (e.key === 'Escape') {
              e.preventDefault();
              ui.set('fsmDrawerOpen', false);
            }
          }}
          style={{
            flex: 1,
            width: '100%',
            resize: 'none',
            border: 'none',
            outline: 'none',
            padding: '12px 14px',
            fontFamily: 'var(--mono)',
            fontSize: '12px',
            lineHeight: 1.5,
            background: 'var(--panel)',
            color: 'var(--text)',
          }}
        />
      </div>
    </div>
  );
}

function Toast() {
  const t = ui.toast;
  if (!t) return null;
  return (
    <div class="toast" role="status" style={t.kind === 'error' ? { borderColor: 'var(--danger)', color: 'var(--danger)' } : undefined} data-testid="toast">
      {t.text}
    </div>
  );
}

// The document-name field in the toolbar head. Keeps a local draft so typing neither
// mutates the document nor re-renders the app; commits once on blur or Enter, and Escape
// cancels via a synchronous flag the trailing blur honours (the cancel-ref idiom used by
// NumberInput and the setup fields). Re-syncs from the store when not dirty so undo, redo
// and external loads update the visible name.
function DocNameField() {
  const [v, setV] = useState(store.doc.name);
  const [dirty, setDirty] = useState(false);
  const cancelRef = useRef(false);
  useEffect(() => {
    if (!dirty) setV(store.doc.name);
  }, [store.doc.name, dirty]);
  const commit = () => {
    if (cancelRef.current) {
      cancelRef.current = false;
      setDirty(false);
      return;
    }
    const t = v.trim() || 'Untitled';
    if (dirty && t !== store.doc.name) store.transact('Rename document', (d) => (d.name = t));
    setDirty(false);
  };
  return (
    <input
      class="doc-name"
      data-testid="doc-name"
      value={v}
      aria-label="Document name"
      spellcheck={false}
      onFocus={() => {
        cancelRef.current = false;
      }}
      onInput={(e) => {
        setV((e.target as HTMLInputElement).value);
        setDirty(true);
      }}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === 'Escape') {
          cancelRef.current = true;
          setV(store.doc.name);
          setDirty(false);
          (e.target as HTMLInputElement).blur();
        }
        if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
      }}
      onBlur={commit}
    />
  );
}

export function App() {
  useApp();
  return (
    <div class={`app${ui.treeCollapsed ? ' tree-collapsed' : ''}${ui.inspCollapsed ? ' insp-collapsed' : ''}`}>
      <aside class={`panel tree-panel${ui.treeOpen ? ' open' : ''}`}>
        <div class="panel-head">
          <DocNameField />
          <button class="btn ghost icon" title={`Undo ${store.undoLabel()} (Ctrl Z)`} disabled={!store.canUndo()} onClick={() => store.undo()} data-testid="undo">
            <Icon.undo />
          </button>
          <button class="btn ghost icon" title={`Redo ${store.redoLabel()} (Ctrl Shift Z)`} disabled={!store.canRedo()} onClick={() => store.redo()} data-testid="redo">
            <Icon.redo />
          </button>
          <button class="btn ghost icon" title="New document" onClick={newDocument} data-testid="new-doc">
            <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">
              <path d="M3.5 1.5h6l3 3v10h-9v-13Z" />
              <path d="M8 6v4M6 8h4" />
            </svg>
          </button>
          <button class="btn ghost icon" title="Open .fsm / .fsmz (Ctrl O)" onClick={openFile} data-testid="open">
            <Icon.folder />
          </button>
          <button class="btn ghost icon" title="Import mesh (.stl / .obj / .glb / .3mf)" onClick={importMeshCommand} data-testid="import-mesh">
            <Icon.mesh />
          </button>
          <button class="btn ghost icon" title="Save .fsm (Ctrl S)" onClick={saveFile} data-testid="save">
            <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round">
              <path d="M8 2v8M4.5 6.5 8 10l3.5-3.5M2.5 13.5h11" />
            </svg>
          </button>
          <button
            class="btn ghost small drawer-only-btn"
            title="Switch directly to Inspector settings"
            data-testid="tree-switch-to-insp"
            onClick={() => {
              ui.set('treeOpen', false);
              ui.set('inspOpen', true);
            }}
            style={{ fontSize: '11px', padding: '0 6px', height: '24px' }}
          >
            ⚙ Edit →
          </button>
          <button
            class="btn ghost icon desktop-only-btn"
            title="Collapse left sidebar ([)"
            data-testid="collapse-tree-btn"
            onClick={() => runCommand('view.toggleTree')}
          >
            <Icon.sidebarLeft />
          </button>
          <button
            class="btn ghost icon drawer-only-btn"
            title="Close Shapes drawer"
            data-testid="close-tree-drawer"
            onClick={() => ui.set('treeOpen', false)}
            style={{ fontWeight: 700, fontSize: '13px' }}
          >
            ✕
          </button>
        </div>
        <div class="panel-body">
          <Tree />
        </div>
      </aside>
      <main class="view">
        <ViewportHost />
        {ui.treeCollapsed && (
          <button
            class="btn icon expand-tree-btn"
            data-testid="expand-tree-btn"
            title="Show Parts & Regions sidebar ([)"
            onClick={() => runCommand('view.toggleTree')}
          >
            <Icon.sidebarLeft />
          </button>
        )}
        <div class="drawer-toggles">
          <button class="btn icon" title="Parts tree" data-testid="toggle-tree" onClick={() => ui.set('treeOpen', !ui.treeOpen)}>
            <Icon.menu />
          </button>
          <button class="btn icon" title="Inspector" data-testid="toggle-insp" onClick={() => ui.set('inspOpen', !ui.inspOpen)}>
            <Icon.sliders />
          </button>
        </div>
        {(ui.treeOpen || ui.inspOpen) && (
          <div
            class="drawer-scrim"
            data-testid="drawer-scrim"
            onClick={() => {
              ui.set('treeOpen', false);
              ui.set('inspOpen', false);
            }}
          />
        )}
        <Lenses />
        <ViewTools />
        <EmptyState />
        <RenderErrorSurface />
        {viewOverlays.map((O, i) => (
          <O key={i} />
        ))}
        <LazyMount
          active={ui.systemsOpen || matchedCoupledSystemIdForDoc(store.doc) !== null}
          load={() => import('./systems').then((m) => m.SystemsOverlayHost)}
        />
        <LazyMount
          active={ui.tutorialsOpen || ui.mcpSetupOpen}
          load={() => import('./tutorials').then((m) => m.TutorialsOverlayHost)}
        />
        <Toast />
      </main>
      <aside class={`panel insp-panel${ui.inspOpen ? ' open' : ''}`}>
        <div class="panel-head">
          <span class="title">Inspector</span>
          <button
            class="btn ghost small drawer-only-btn"
            title="Switch to Shapes & Boundary Tree"
            data-testid="insp-switch-to-tree"
            onClick={() => {
              ui.set('inspOpen', false);
              ui.set('treeOpen', true);
            }}
            style={{ fontSize: '11px', padding: '0 6px', height: '24px' }}
          >
            ← Tree
          </button>
          <button class="btn ghost small" title="Command palette (Ctrl K)" onClick={() => ui.set('paletteOpen', true)} data-testid="palette-button">
            <span class="kbd">Ctrl K</span>
          </button>
          <button
            class="btn ghost icon desktop-only-btn"
            title="Collapse Inspector sidebar (])"
            data-testid="collapse-insp-btn"
            onClick={() => runCommand('view.toggleInsp')}
          >
            <Icon.sidebarRight />
          </button>
          <button
            class="btn ghost icon drawer-only-btn"
            title="Close Inspector drawer"
            data-testid="close-insp-drawer"
            onClick={() => ui.set('inspOpen', false)}
            style={{ fontWeight: 700, fontSize: '13px' }}
          >
            ✕
          </button>
        </div>
        <div class="panel-body">
          <Inspector />
        </div>
      </aside>
      <StatusBar />
      <Palette />
      <Menus />
      <FsmSourceDrawer />
      <LazyMount
        active={ui.sketchOpen}
        load={() => import('./SketchWorkbench').then((m) => m.SketchWorkbenchModal)}
      />
      <LazyMount
        active={ui.fixingsOpen}
        load={() => import('./FixingsModal').then((m) => m.FixingsCatalogueModal)}
      />
      <LazyMount
        active={ui.antennaOpen}
        load={() => import('./AntennaWorkbench').then((m) => m.AntennaWorkbenchModal)}
      />
      <LazyMount
        active={ui.channelProfileOpen}
        load={() => import('./ChannelProfileModal').then((m) => m.ChannelProfileModal)}
      />
      <LazyMount
        active={ui.freeSurfaceOpen}
        load={() => import('./FreeSurfaceModal').then((m) => m.FreeSurfaceModal)}
      />
      <LazyMount
        active={ui.coordMappingOpen}
        load={() => import('./CoordMappingModal').then((m) => m.CoordMappingModal)}
      />
    </div>
  );
}
