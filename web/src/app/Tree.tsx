// Model tree: the ordered list of shapes (and, in the Setup lens, the analysis items).
import { useLayoutEffect, useRef, useState } from 'preact/hooks';
import type { JSX } from 'preact';
import { store } from '../doc/store';
import { findNode, kindLabel } from '../doc/types';
import type { Node, Op } from '../doc/types';
import { duplicateNode, moveNode, renameId, inheritedColor, removeNodeAndCompanion } from '../doc/ops';
import { Icon, ShapeIcon } from './icons';
import { ui, useApp } from './ui';
import { openFixingsCatalogueModal } from './FixingsModal';
import { openSketchWorkbenchModal } from './SketchWorkbench';
import { openAntennaWorkbenchModal } from './AntennaWorkbench';
import { extractDroneAntennaSpecFromDoc } from '../doc/antenna-vna';
import { addShape } from './commands';

const OP_GLYPH = { add: '+', subtract: '−', intersect: '∩' } as const;
const OP_TITLE = { add: 'Adds material', subtract: 'Cuts material', intersect: 'Keeps only the overlap' } as const;

export function RenameInput({ id, onDone }: { id: string; onDone: () => void }) {
  const ref = useRef<HTMLInputElement>(null);
  const doneRef = useRef(false);
  const [value, setValue] = useState(id);
  useLayoutEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);
  const commit = () => {
    if (doneRef.current) return;
    doneRef.current = true;
    const to = value.trim();
    if (to && to !== id) {
      let err: string | null = null;
      store.transact(`Rename ${id} to ${to}`, (d) => {
        err = renameId(d, id, to);
      });
      if (err) {
        doneRef.current = false;
        ui.notify(err, 'error');
        return;
      }
      const s = store.selection;
      if (s && s.id === id) store.select({ ...s, id: to });
    }
    onDone();
  };
  return (
    <input
      ref={ref}
      value={value}
      spellcheck={false}
      onInput={(e) => setValue((e.target as HTMLInputElement).value)}
      onKeyDown={(e) => {
        if (e.key === 'Enter') commit();
        if (e.key === 'Escape') {
          doneRef.current = true;
          onDone();
        }
        e.stopPropagation();
      }}
      onBlur={commit}
      onClick={(e) => e.stopPropagation()}
    />
  );
}

let dragId: string | null = null;

function NodeRow({ node, depth }: { node: Node; depth: number }) {
  const [drop, setDrop] = useState<'' | 'before' | 'after' | 'inside'>('');
  const sel = store.selection;
  const selected = sel?.kind === 'node' && sel.id === node.id;
  const isGroup = node.kind.type === 'group';
  const collapsed = ui.collapsed.has(node.id);
  const children = node.kind.type === 'group' ? node.kind.children : [];
  const rowColour = node.color ?? inheritedColor(store.doc, node.id)?.color;

  const zone = (e: DragEvent): 'before' | 'after' | 'inside' => {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const y = (e.clientY - r.top) / r.height;
    if (isGroup && y > 0.3 && y < 0.7) return 'inside';
    return y < 0.5 ? 'before' : 'after';
  };

  const NEXT_OP: Record<Op, Op> = { add: 'subtract', subtract: 'intersect', intersect: 'add' };
  const row: JSX.Element = (
    <div
      class={`row op-row-${node.op} ${selected ? 'selected' : ''} ${node.suppressed ? 'suppressed' : ''} ${drop ? `drop-${drop}` : ''}`}
      style={{
        paddingLeft: `${8 + depth * 14}px`,
        ...(node.op === 'subtract' && !selected
          ? { background: 'rgba(239, 68, 68, 0.045)', borderLeft: '2px dashed rgba(239, 68, 68, 0.55)' }
          : {}),
      }}
      draggable
      role="treeitem"
      aria-selected={selected}
      aria-label={`${node.id}, ${kindLabel(node.kind)}${node.suppressed ? ', suppressed' : ''}`}
      data-node={node.id}
      onDragStart={(e) => {
        dragId = node.id;
        e.dataTransfer?.setData('text/plain', node.id);
      }}
      onDragOver={(e) => {
        if (!dragId || dragId === node.id) return;
        e.preventDefault();
        setDrop(zone(e));
      }}
      onDragLeave={() => setDrop('')}
      onDrop={(e) => {
        e.preventDefault();
        const where = zone(e);
        setDrop('');
        const id = dragId;
        dragId = null;
        if (!id) return;
        let ok = true;
        store.transact(`Move ${id}`, (d) => {
          ok = moveNode(d, id, node.id, where);
        });
        if (!ok) ui.notify('A group cannot be moved inside itself.', 'error');
      }}
      onClick={() => {
        store.select({ kind: 'node', id: node.id });
        if (typeof window !== 'undefined' && window.innerWidth <= 900 && store.lens === 'shape') {
          ui.set('treeOpen', false);
          ui.set('inspOpen', true);
        }
      }}
      onDblClick={() => ui.set('renaming', node.id)}
      title={`${kindLabel(node.kind)} · ${OP_TITLE[node.op]} (Tap to edit settings)`}
    >
      {isGroup ? (
        <span
          class="twisty"
          onClick={(e) => {
            e.stopPropagation();
            if (collapsed) ui.collapsed.delete(node.id);
            else ui.collapsed.add(node.id);
            ui.emit();
          }}
          style={{ transform: collapsed ? '' : 'rotate(90deg)' }}
        >
          <Icon.chevron />
        </span>
      ) : (
        <span class="twisty" />
      )}
      <button
        type="button"
        class={`op ${node.op}`}
        data-testid={`cycle-op-${node.id}`}
        title={`${OP_TITLE[node.op]} — click to cycle Add (+) / Cut (−) / Intersect (∩)`}
        onClick={(e) => {
          e.stopPropagation();
          const next = NEXT_OP[node.op];
          store.transact(`Set ${node.id} to ${next}`, (d) => {
            const target = findNode(d, node.id);
            if (target) target.op = next;
          });
        }}
        style={{
          background: node.op === 'subtract' ? 'rgba(239, 68, 68, 0.16)' : 'transparent',
          border: 'none',
          borderRadius: '3px',
          padding: '0 3px',
          cursor: 'pointer',
          fontFamily: 'var(--mono)',
          fontWeight: 700,
        }}
      >
        {OP_GLYPH[node.op]}
      </button>
      <span class="glyph">
        <ShapeIcon type={node.kind.type} />
      </span>
      {rowColour && node.op !== 'subtract' && (
        <span class="swatch" style={{ background: rowColour }} title={node.color ? `Own colour ${node.color}` : `Colour ${rowColour} inherited`} />
      )}
      <span class="row-name">{ui.renaming === node.id ? <RenameInput id={node.id} onDone={() => ui.set('renaming', null)} /> : node.id}</span>
      {node.blend > 0 && <span class="meta" title="Blend radius">r{node.blend}</span>}
      <span class="row-actions">
        <button
          class="mini drawer-only-btn"
          title={`Edit ${node.id} settings in Inspector`}
          data-testid={`edit-node-${node.id}`}
          onClick={(e) => {
            e.stopPropagation();
            store.select({ kind: 'node', id: node.id });
            ui.set('treeOpen', false);
            ui.set('inspOpen', true);
            ui.set('inspCollapsed', false);
          }}
        >
          <Icon.sliders />
        </button>
        <button
          class="mini"
          title={`Duplicate ${node.id} (Ctrl D)`}
          data-testid={`duplicate-node-${node.id}`}
          onClick={(e) => {
            e.stopPropagation();
            let copy: Node | null = null;
            store.transact(`Duplicate ${node.id}`, (d) => {
              copy = duplicateNode(d, node.id);
            });
            if (copy) store.select({ kind: 'node', id: (copy as Node).id });
          }}
        >
          ⧉
        </button>
        <button
          class="mini drawer-only-btn"
          title={`Rename ${node.id} (F2)`}
          data-testid={`rename-node-${node.id}`}
          onClick={(e) => {
            e.stopPropagation();
            store.select({ kind: 'node', id: node.id });
            ui.set('renaming', node.id);
          }}
        >
          <Icon.edit />
        </button>
        <button
          class="mini"
          title={node.suppressed ? 'Unsuppress (H)' : 'Suppress (H)'}
          onClick={(e) => {
            e.stopPropagation();
            store.transact(`${node.suppressed ? 'Unsuppress' : 'Suppress'} ${node.id}`, (d) => {
              const walkFind = (l: Node[]): Node | null => {
                for (const n of l) {
                  if (n.id === node.id) return n;
                  if (n.kind.type === 'group') {
                    const r = walkFind(n.kind.children);
                    if (r) return r;
                  }
                }
                return null;
              };
              const n = walkFind(d.root);
              if (n) n.suppressed = !n.suppressed;
            });
          }}
        >
          {node.suppressed ? <Icon.eyeOff /> : <Icon.eye />}
        </button>
        <button
          class="mini"
          title={
            isGroup
              ? `Delete group ${node.id} and all ${children.length} child object(s) (Del)`
              : `Delete ${node.id} (Del)`
          }
          data-testid={`delete-node-${node.id}`}
          onClick={(e) => {
            e.stopPropagation();
            store.transact(`Delete ${node.id}`, (d) => {
              removeNodeAndCompanion(d, node.id, !node.id.endsWith('_bore'));
            });
            if (store.selection?.kind === 'node' && store.selection.id === node.id) {
              store.select(null);
            }
          }}
          style={{ color: 'var(--bad, #ef4444)' }}
        >
          <Icon.trash />
        </button>
      </span>
    </div>
  );

  return (
    <>
      {row}
      {isGroup && !collapsed && children.map((c) => <NodeRow key={c.id} node={c} depth={depth + 1} />)}
    </>
  );
}

/** Extra tree sections contributed by other lenses (Setup lists regions, supports, loads). */
export const treeSections: (() => JSX.Element | null)[] = [];

export function Tree() {
  useApp();
  const doc = store.doc;
  return (
    <div class="tree" role="tree" aria-label="Model tree" onClick={(e) => e.target === e.currentTarget && store.select(null)}>
      <div class="section-label">
        <span>Shapes</span>
        <div style={{ display: 'flex', alignItems: 'center', gap: '4px' }}>
          <button
            class="btn ghost small"
            title="Open Parametric 2D Sketch & Surface-Feature Workbench (LM + QR Constraint Solver)"
            data-testid="open-sketch-workbench"
            onClick={() => openSketchWorkbenchModal()}
            style={{ fontSize: '11px', padding: '2px 7px', display: 'inline-flex', alignItems: 'center', gap: '4px' }}
          >
            <Icon.sketcher /> Sketch…
          </button>
          <button
            class="btn ghost small"
            title="Open Built-In Engineering Fixings & Snap-Fit Geometry Catalogue (Auto-Tap Host)"
            data-testid="open-fixings-catalog"
            onClick={() => openFixingsCatalogueModal()}
            style={{ fontSize: '11px', padding: '2px 7px', display: 'inline-flex', alignItems: 'center', gap: '4px' }}
          >
            <Icon.fixing /> Fixings…
          </button>
          {extractDroneAntennaSpecFromDoc(store.doc) && (
            <button
              class="btn ghost small"
              title="Open RF Antenna VNA / Smith Chart / 3D Lobe Workbench"
              data-testid="open-antenna-workbench"
              onClick={() => openAntennaWorkbenchModal()}
              style={{ fontSize: '11px', padding: '2px 7px', display: 'inline-flex', alignItems: 'center', gap: '4px' }}
            >
              <Icon.antenna /> Antenna…
            </button>
          )}
          <button
            class="btn ghost icon small"
            title="Add shape (Shift B, Shift C, …)"
            data-action="add-menu"
            onClick={(e) => {
              const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
              ui.set('menu', { kind: 'add', x: r.left, y: r.bottom + 4 });
            }}
          >
            <Icon.plus />
          </button>
        </div>
      </div>
      <div
        data-testid="tree-quick-add-bar"
        style={{
          display: 'flex',
          flexWrap: 'wrap',
          gap: '4px',
          padding: '2px 10px 6px',
          borderBottom: '1px solid var(--line)',
          marginBottom: '4px',
        }}
      >
        <button
          type="button"
          class="btn small"
          data-testid="tree-quick-add-box"
          title="Add a Box and open its settings immediately"
          onClick={() => addShape('box')}
          style={{ fontSize: '11px', padding: '0 7px', height: '24px' }}
        >
          + Box
        </button>
        <button
          type="button"
          class="btn small"
          data-testid="tree-quick-add-cylinder"
          title="Add a Cylinder and open its settings immediately"
          onClick={() => addShape('cylinder')}
          style={{ fontSize: '11px', padding: '0 7px', height: '24px' }}
        >
          + Cyl
        </button>
        <button
          type="button"
          class="btn small"
          data-testid="tree-quick-add-sphere"
          title="Add a Sphere and open its settings immediately"
          onClick={() => addShape('sphere')}
          style={{ fontSize: '11px', padding: '0 7px', height: '24px' }}
        >
          + Sphere
        </button>
        <button
          type="button"
          class="btn small"
          data-testid="tree-quick-add-cut-hole"
          title="Add a Boolean Cut Cylinder (hole) and open its settings immediately"
          onClick={() => addShape('cylinder', { op: 'subtract' })}
          style={{
            fontSize: '11px',
            padding: '0 7px',
            height: '24px',
            color: 'var(--danger)',
            borderColor: 'color-mix(in srgb, var(--danger) 45%, var(--line-strong))',
          }}
        >
          − Cut Hole
        </button>
      </div>
      {doc.root.length === 0 && <div class="hint">No shapes yet. Tap + Box / + Cyl above, or press Ctrl K.</div>}
      {doc.root.map((n) => (
        <NodeRow key={n.id} node={n} depth={0} />
      ))}
      {treeSections.map((S, i) => (
        <S key={i} />
      ))}
    </div>
  );
}
