// Model tree: the ordered list of shapes (and, in the Setup lens, the analysis items).
import { useLayoutEffect, useRef, useState } from 'preact/hooks';
import type { JSX } from 'preact';
import { store } from '../doc/store';
import { kindLabel } from '../doc/types';
import type { Node } from '../doc/types';
import { moveNode, renameId, inheritedColor } from '../doc/ops';
import { Icon, ShapeIcon } from './icons';
import { ui, useApp } from './ui';

const OP_GLYPH = { add: '+', subtract: '−', intersect: '∩' } as const;
const OP_TITLE = { add: 'Adds material', subtract: 'Cuts material', intersect: 'Keeps only the overlap' } as const;

export function RenameInput({ id, onDone }: { id: string; onDone: () => void }) {
  const ref = useRef<HTMLInputElement>(null);
  const [value, setValue] = useState(id);
  useLayoutEffect(() => {
    ref.current?.focus();
    ref.current?.select();
  }, []);
  const commit = () => {
    const to = value.trim();
    if (to && to !== id) {
      let err: string | null = null;
      store.transact(`Rename ${id} to ${to}`, (d) => {
        err = renameId(d, id, to);
      });
      if (err) {
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
        if (e.key === 'Escape') onDone();
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

  const row: JSX.Element = (
    <div
      class={`row ${selected ? 'selected' : ''} ${node.suppressed ? 'suppressed' : ''} ${drop ? `drop-${drop}` : ''}`}
      style={{ paddingLeft: `${8 + depth * 14}px` }}
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
      onClick={() => store.select({ kind: 'node', id: node.id })}
      onDblClick={() => ui.set('renaming', node.id)}
      title={`${kindLabel(node.kind)} · ${OP_TITLE[node.op]}`}
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
      <span class={`op ${node.op}`} title={OP_TITLE[node.op]}>
        {OP_GLYPH[node.op]}
      </span>
      <span class="glyph">
        <ShapeIcon type={node.kind.type} />
      </span>
      {rowColour && (
        <span class="swatch" style={{ background: rowColour }} title={node.color ? `Own colour ${node.color}` : `Colour ${rowColour} inherited`} />
      )}
      <span class="row-name">{ui.renaming === node.id ? <RenameInput id={node.id} onDone={() => ui.set('renaming', null)} /> : node.id}</span>
      {node.blend > 0 && <span class="meta" title="Blend radius">r{node.blend}</span>}
      <span class="row-actions">
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
      {doc.root.length === 0 && <div class="hint">No shapes yet. Add one with the + button, or press Ctrl K.</div>}
      {doc.root.map((n) => (
        <NodeRow key={n.id} node={n} depth={0} />
      ))}
      {treeSections.map((S, i) => (
        <S key={i} />
      ))}
    </div>
  );
}
