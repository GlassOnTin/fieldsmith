// The application shell: tree | viewport | inspector, status line below.
import type { JSX } from 'preact';
import { store, LENSES } from '../doc/store';
import { FACE_LABEL } from '../doc/types';
import type { Face } from '../doc/types';
import { Tree } from './Tree';
import { Inspector } from './Inspector';
import { ViewportHost } from './ViewportHost';
import { Palette, Menus } from './Palette';
import { EXAMPLES, addShape, importMeshCommand, newDocument, openFile, openText, runCommand, saveFile } from './commands';
import { Icon } from './icons';
import { ui, useApp } from './ui';

/** Lens-specific viewport overlays and status items (registered by later lenses). */
export const viewOverlays: (() => JSX.Element | null)[] = [];
export const statusItems: (() => JSX.Element | null)[] = [];

function Lenses() {
  return (
    <div class="lenses" role="tablist" aria-label="Lens">
      {LENSES.map((l) => (
        <button key={l.id} role="tab" class={store.lens === l.id ? 'on' : ''} aria-selected={store.lens === l.id} data-lens={l.id} title={`${l.label} (${l.key})`} onClick={() => store.setLens(l.id)}>
          {l.label}
          <span class="kbd">{l.key}</span>
        </button>
      ))}
    </div>
  );
}

function ViewTools() {
  return (
    <div class="view-tools">
      <button class="btn icon" title="Fit model (F)" onClick={() => runCommand('view.fit')}>
        <Icon.fit />
      </button>
      {store.lens === 'shape' && (
        <>
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
  if (store.doc.root.length > 0) return null;
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
        <p style={{ margin: '14px 0 6px', fontSize: '12px' }}>Or open an example:</p>
        <div class="actions">
          {EXAMPLES.map((e) => (
            <button key={e.id} class="btn small" onClick={() => openText(e.text, `${e.id}.fsm`)} data-testid={`example-${e.id}`}>
              {e.label}
            </button>
          ))}
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
      {s && (
        <span title="Deselect with Esc, a right-click in the viewport, or clicking empty space">
          Selected: {s.id} · Esc / right-click / empty space to deselect
        </span>
      )}
      <span class="sep">|</span>
      <span class="num" title="Render time of the last frames">
        {ui.frameMs.toFixed(1)} ms{fps ? ` · ${fps} fps max` : ''}
      </span>
      <span class="sep">|</span>
      <span>mm · N · MPa</span>
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

export function App() {
  useApp();
  return (
    <div class="app">
      <aside class="panel tree-panel">
        <div class="panel-head">
          <input
            class="doc-name"
            value={store.doc.name}
            aria-label="Document name"
            spellcheck={false}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
            }}
            onChange={(e) => {
              const v = (e.target as HTMLInputElement).value.trim() || 'Untitled';
              store.transact('Rename document', (d) => (d.name = v));
            }}
          />
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
        </div>
        <div class="panel-body">
          <Tree />
        </div>
      </aside>
      <main class="view">
        <ViewportHost />
        <Lenses />
        <ViewTools />
        <EmptyState />
        {viewOverlays.map((O, i) => (
          <O key={i} />
        ))}
        <Toast />
      </main>
      <aside class="panel insp-panel">
        <div class="panel-head">
          <span class="title">Inspector</span>
          <button class="btn ghost small" title="Command palette (Ctrl K)" onClick={() => ui.set('paletteOpen', true)} data-testid="palette-button">
            <span class="kbd">Ctrl K</span>
          </button>
        </div>
        <div class="panel-body">
          <Inspector />
        </div>
      </aside>
      <StatusBar />
      <Palette />
      <Menus />
    </div>
  );
}
