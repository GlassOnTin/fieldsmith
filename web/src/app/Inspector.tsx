// Inspector: exact values for whatever is selected (or the document when nothing is).
import type { JSX } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import { store } from '../doc/store';
import { findNode, kindLabel } from '../doc/types';
import type { Document, Modifier, Node, NodeKind, Op } from '../doc/types';
import { materialLibrary, validateWithWarnings } from '../doc/wasm';
import { MODIFIER_LABEL, defaultModifier, renameId } from '../doc/ops';
import { NumRow, NumberInput, Seg, VecRow } from './fields';
import { Icon } from './icons';
import { ui, useApp } from './ui';

let library: ReturnType<typeof materialLibrary> | null = null;
export function materials() {
  if (!library) library = materialLibrary();
  return library;
}

/** Address a node in a draft by id (throws if missing — callers only run for live selections). */
function nodeIn(d: Document, id: string): Node {
  const n = findNode(d, id);
  if (!n) throw new Error(`missing node ${id}`);
  return n;
}

function kindIn<T extends NodeKind['type']>(d: Document, id: string, _t: T): Extract<NodeKind, { type: T }> {
  return nodeIn(d, id).kind as Extract<NodeKind, { type: T }>;
}

function Dimensions({ node }: { node: Node }) {
  const id = node.id;
  const u = `Edit ${id}`;
  switch (node.kind.type) {
    case 'box':
      return (
        <>
          <VecRow label="Size" unit="mm" min={0.01} get={(d) => kindIn(d, id, 'box').size} set={(d, v) => (kindIn(d, id, 'box').size = v)} undoLabel={u} testId="size" />
          <NumRow label="Corner radius" unit="mm" min={0} step={0.5} get={(d) => kindIn(d, id, 'box').round} set={(d, v) => (kindIn(d, id, 'box').round = v)} undoLabel={u} testId="round" />
        </>
      );
    case 'cylinder':
      return (
        <>
          <NumRow label="Diameter" unit="mm" min={0.01} step={0.5} get={(d) => kindIn(d, id, 'cylinder').radius * 2} set={(d, v) => (kindIn(d, id, 'cylinder').radius = v / 2)} undoLabel={u} testId="diameter" />
          <NumRow label="Height" unit="mm" min={0.01} get={(d) => kindIn(d, id, 'cylinder').height} set={(d, v) => (kindIn(d, id, 'cylinder').height = v)} undoLabel={u} testId="height" />
          <NumRow label="Edge radius" unit="mm" min={0} step={0.5} get={(d) => kindIn(d, id, 'cylinder').round} set={(d, v) => (kindIn(d, id, 'cylinder').round = v)} undoLabel={u} />
        </>
      );
    case 'sphere':
      return <NumRow label="Diameter" unit="mm" min={0.01} get={(d) => kindIn(d, id, 'sphere').radius * 2} set={(d, v) => (kindIn(d, id, 'sphere').radius = v / 2)} undoLabel={u} testId="diameter" />;
    case 'capsule':
      return (
        <>
          <NumRow label="Diameter" unit="mm" min={0.01} get={(d) => kindIn(d, id, 'capsule').radius * 2} set={(d, v) => (kindIn(d, id, 'capsule').radius = v / 2)} undoLabel={u} />
          <NumRow label="Length" unit="mm" min={0} title="Straight length between the hemispherical ends" get={(d) => kindIn(d, id, 'capsule').length} set={(d, v) => (kindIn(d, id, 'capsule').length = v)} undoLabel={u} />
        </>
      );
    case 'torus':
      return (
        <>
          <NumRow label="Ring radius" unit="mm" min={0.01} get={(d) => kindIn(d, id, 'torus').major} set={(d, v) => (kindIn(d, id, 'torus').major = v)} undoLabel={u} />
          <NumRow label="Tube radius" unit="mm" min={0.01} step={0.5} get={(d) => kindIn(d, id, 'torus').minor} set={(d, v) => (kindIn(d, id, 'torus').minor = v)} undoLabel={u} />
        </>
      );
    case 'extrude':
      return (
        <>
          <NumRow label="Height" unit="mm" min={0.01} get={(d) => kindIn(d, id, 'extrude').height} set={(d, v) => (kindIn(d, id, 'extrude').height = v)} undoLabel={u} />
        </>
      );
    case 'mesh':
      return null;
    case 'extend': {
      const k = node.kind;
      return (
        <>
          <label class="plain">Region</label>
          <select
            class="select"
            data-testid="extend-region"
            value={k.region}
            onChange={(e) => store.transact(`Set ${id} region`, (d) => (kindIn(d, id, 'extend').region = (e.target as HTMLSelectElement).value))}
          >
            <option value="">—</option>
            {store.doc.regions.map((r) => (
              <option key={r.id} value={r.id}>
                {r.id}
              </option>
            ))}
          </select>
          <VecRow label="Direction" step={0.5} get={(d) => kindIn(d, id, 'extend').dir} set={(d, v) => (kindIn(d, id, 'extend').dir = v)} undoLabel={u} testId="extend-dir" />
          <NumRow label="Length" unit="mm" min={0.01} step={1} get={(d) => kindIn(d, id, 'extend').len} set={(d, v) => (kindIn(d, id, 'extend').len = v)} undoLabel={u} testId="extend-len" />
        </>
      );
    }
    case 'group':
      return null;
  }
}

function ProfileEditor({ node }: { node: Node }) {
  if (node.kind.type !== 'extrude') return null;
  const id = node.id;
  const pts = node.kind.profile;
  const edit = (label: string, f: (p: { x: number; y: number; bulge: number }[]) => void) =>
    store.transact(label, (d) => f(kindIn(d, id, 'extrude').profile));
  return (
    <div class="card">
      <div class="card-head">
        <span>Profile</span>
        <button
          class="btn ghost small"
          title="Add a point after the last one"
          onClick={() =>
            edit('Add profile point', (p) => {
              const a = p[p.length - 1];
              const b = p[0];
              p.push({ x: (a.x + b.x) / 2, y: (a.y + b.y) / 2, bulge: 0 });
            })
          }
        >
          Add point
        </button>
      </div>
      <div style={{ padding: '6px 8px' }}>
        <table class="profile-table">
          <thead>
            <tr>
              <td />
              <td class="hint" style={{ padding: 0 }}>x</td>
              <td class="hint" style={{ padding: 0 }}>y</td>
              <td class="hint" style={{ padding: 0 }} title="Arc to the next point: 0 = straight, 1 = semicircle, negative bends the other way">
                arc
              </td>
              <td />
            </tr>
          </thead>
          <tbody>
            {pts.map((p, i) => (
              <tr key={i}>
                <td class="idx">{i + 1}</td>
                <td>
                  <NumberInput value={p.x} unit="mm" onChange={(v) => edit('Edit profile', (q) => (q[i].x = v))} />
                </td>
                <td>
                  <NumberInput value={p.y} unit="mm" onChange={(v) => edit('Edit profile', (q) => (q[i].y = v))} />
                </td>
                <td>
                  <NumberInput value={p.bulge} step={0.1} min={-1} max={1} onChange={(v) => edit('Edit profile', (q) => (q[i].bulge = v))} />
                </td>
                <td>
                  <button class="btn ghost icon small" title="Remove point" disabled={pts.length <= 3} onClick={() => edit('Remove profile point', (q) => q.splice(i, 1))}>
                    <Icon.trash />
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}

function ModifierCard({ node, m, index }: { node: Node; m: Modifier; index: number }) {
  const id = node.id;
  const mods = (d: Document) => (kindIn(d, id, 'group') as Extract<NodeKind, { type: 'group' }>).modifiers;
  const u = `Edit ${MODIFIER_LABEL[m.type].toLowerCase()}`;
  const get = <T extends Modifier['type']>(d: Document) => mods(d)[index] as Extract<Modifier, { type: T }>;
  let body: JSX.Element | null = null;
  switch (m.type) {
    case 'shell':
      body = (
        <div class="field-grid">
          <NumRow label="Thickness" unit="mm" min={0.01} step={0.1} get={(d) => get<'shell'>(d).thickness} set={(d, v) => (get<'shell'>(d).thickness = v)} undoLabel={u} />
          <label class="plain">Side</label>
          <Seg
            value={m.side}
            options={[
              { value: 'inside', label: 'Inward' },
              { value: 'centre', label: 'Centred' },
              { value: 'outside', label: 'Outward' },
            ]}
            onChange={(v) => store.transact(u, (d) => (get<'shell'>(d).side = v))}
          />
        </div>
      );
      break;
    case 'offset':
      body = (
        <div class="field-grid">
          <NumRow label="Distance" unit="mm" step={0.1} title="Positive grows, negative shrinks" get={(d) => get<'offset'>(d).distance} set={(d, v) => (get<'offset'>(d).distance = v)} undoLabel={u} />
        </div>
      );
      break;
    case 'mirror':
      body = (
        <div class="field-grid">
          <label class="plain">Across</label>
          <Seg
            value={m.axis}
            options={[
              { value: 'x', label: 'YZ plane' },
              { value: 'y', label: 'XZ plane' },
              { value: 'z', label: 'XY plane' },
            ]}
            onChange={(v) => store.transact(u, (d) => (get<'mirror'>(d).axis = v))}
          />
        </div>
      );
      break;
    case 'linear':
      body = (
        <div class="field-grid">
          <NumRow label="Count" min={1} max={64} integer get={(d) => get<'linear'>(d).count} set={(d, v) => (get<'linear'>(d).count = v)} undoLabel={u} />
          <VecRow label="Step" unit="mm" get={(d) => get<'linear'>(d).step} set={(d, v) => (get<'linear'>(d).step = v)} undoLabel={u} />
        </div>
      );
      break;
    case 'circular':
      body = (
        <div class="field-grid">
          <NumRow label="Count" min={1} max={64} integer get={(d) => get<'circular'>(d).count} set={(d, v) => (get<'circular'>(d).count = v)} undoLabel={u} />
          <NumRow label="Angle" unit="°" min={1} max={360} step={15} title="Total sweep; 360 spaces copies evenly round a full turn" get={(d) => get<'circular'>(d).angle} set={(d, v) => (get<'circular'>(d).angle = v)} undoLabel={u} />
          <label class="plain">Axis</label>
          <Seg
            value={m.axis}
            options={[
              { value: 'x', label: 'X' },
              { value: 'y', label: 'Y' },
              { value: 'z', label: 'Z' },
            ]}
            onChange={(v) => store.transact(u, (d) => (get<'circular'>(d).axis = v))}
          />
        </div>
      );
      break;
  }
  return (
    <div class="card" data-modifier={m.type}>
      <div class="card-head">
        <span>{MODIFIER_LABEL[m.type]}</span>
        <button class="btn ghost icon small" title="Remove modifier" onClick={() => store.transact(`Remove ${MODIFIER_LABEL[m.type].toLowerCase()}`, (d) => mods(d).splice(index, 1))}>
          <Icon.trash />
        </button>
      </div>
      {body}
    </div>
  );
}

/** Editable id; renames update every reference. Used for shapes and analysis items. */
export function NameField({ id }: { id: string }) {
  const [text, setText] = useState(id);
  useEffect(() => setText(id), [id]);
  const commit = () => {
    const to = text.trim();
    if (!to || to === id) {
      setText(id);
      return;
    }
    let err: string | null = null;
    store.transact(`Rename ${id} to ${to}`, (d) => {
      err = renameId(d, id, to);
    });
    if (err) {
      ui.notify(err, 'error');
      setText(id);
      return;
    }
    const s = store.selection;
    if (s && s.id === id) store.select({ ...s, id: to });
  };
  return (
    <input
      class="text-input"
      value={text}
      spellcheck={false}
      data-testid="name"
      onFocus={(e) => (e.target as HTMLInputElement).select()}
      onInput={(e) => setText((e.target as HTMLInputElement).value)}
      onBlur={commit}
      onKeyDown={(e) => {
        e.stopPropagation();
        if (e.key === 'Enter') (e.target as HTMLInputElement).blur();
        if (e.key === 'Escape') {
          setText(id);
          (e.target as HTMLInputElement).blur();
        }
      }}
    />
  );
}

/** Read-only facts about an imported mesh — nothing here is editable; resize means re-import.
 * Counts are copied from the import report, not derived: a mesh whose solids disagree has
 * nowhere in this panel to hide it. */
function MeshPanel({ node }: { node: Node }) {
  if (node.kind.type !== 'mesh') return null;
  const k = node.kind;
  const src = k.src.includes('/') || k.src.includes('\\') ? k.src.split(/[\\/]/).pop()! : k.src;
  return (
    <>
      <div class="field-grid" style={{ fontSize: '12px' }}>
        <label class="plain">Source</label>
        <span style={{ color: 'var(--text-3)' }}>{k.src}</span>
        <label class="plain">Units / up</label>
        <span class="num">
          {k.units} / {k.up}
        </span>
        <label class="plain">Voxel</label>
        <span class="num">{k.voxel} mm</span>
        <label class="plain">Triangles</label>
        <span class="num">{k.tris.toLocaleString()}</span>
        <label class="plain">Patches</label>
        <span class="num">{k.patches.toLocaleString()}</span>
        <label class="plain">Open edges</label>
        <span class="num">{k.open_edges.toLocaleString()}</span>
        <label class="plain">Grid key</label>
        <span class="num">{k.hash}</span>
      </div>
      {k.open_edges > 0 && (
        <div class="hint">
          {k.open_edges} open edge{k.open_edges === 1 ? '' : 's'} in “{src}”: the surface has holes, so the
          solid fill and any volume readout may be wrong — check before trusting a solve.
        </div>
      )}
    </>
  );
}

function NodeInspector({ node }: { node: Node }) {
  const id = node.id;
  const u = `Edit ${id}`;
  return (
    <div class="insp" data-inspector="node">
      <div class="insp-heading">
        <span class="kind">{kindLabel(node.kind)}</span>
      </div>
      <div class="field-grid">
        <label class="plain">Name</label>
        <NameField id={id} />
        <label class="plain">Operation</label>
        <Seg<Op>
          value={node.op}
          testId="op"
          options={[
            { value: 'add', label: 'Add', title: 'Union with what comes before' },
            { value: 'subtract', label: 'Cut', title: 'Subtract from what comes before' },
            { value: 'intersect', label: 'Intersect', title: 'Keep only the overlap' },
          ]}
          onChange={(v) => store.transact(`Set ${id} to ${v}`, (d) => (nodeIn(d, id).op = v))}
        />
        <NumRow
          label="Blend"
          unit="mm"
          min={0}
          step={0.5}
          title="Fillet radius where this shape meets what comes before (0 = sharp)"
          get={(d) => nodeIn(d, id).blend}
          set={(d, v) => (nodeIn(d, id).blend = v)}
          undoLabel={u}
          testId="blend"
        />
        <Dimensions node={node} />
      </div>
      <div class="section-label">Placement</div>
      <div class="field-grid">
        <VecRow label="Position" unit="mm" get={(d) => nodeIn(d, id).transform.translate} set={(d, v) => (nodeIn(d, id).transform.translate = v)} undoLabel={`Move ${id}`} testId="position" />
        <VecRow label="Rotation" unit="°" step={15} get={(d) => nodeIn(d, id).transform.rotate} set={(d, v) => (nodeIn(d, id).transform.rotate = v)} undoLabel={`Rotate ${id}`} testId="rotation" />
      </div>
      <ProfileEditor node={node} />
      {node.kind.type === 'mesh' && <MeshPanel node={node} />}
      {node.kind.type === 'group' && (
        <>
          <div class="section-label">
            <span>Modifiers</span>
            <button
              class="btn ghost small"
              data-action="add-modifier"
              title="Add a modifier to this group"
              onClick={(e) => {
                const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
                ui.set('menu', { kind: 'modifier', x: r.left - 100, y: r.bottom + 4, target: id });
              }}
            >
              Add
            </button>
          </div>
          {node.kind.modifiers.length === 0 && <div class="hint">Shell, offset, mirror or pattern everything in this group.</div>}
          {node.kind.modifiers.map((m, i) => (
            <ModifierCard key={i} node={node} m={m} index={i} />
          ))}
        </>
      )}
      <div class="field-grid" style={{ marginTop: '8px' }}>
        <label class="plain">Material</label>
        <select
          class="select"
          value={node.material ?? ''}
          data-testid="node-material"
          title="For the slicer with the 3MF export; the solve still evaluates the whole part with the document material"
          onChange={(e) =>
            store.transact(`Label ${id}`, (d) => {
              const v = (e.target as HTMLSelectElement).value;
              if (v === '') delete nodeIn(d, id).material;
              else nodeIn(d, id).material = v;
            })
          }
        >
          <option value="">Document default</option>
          {[...materials(), ...store.doc.custom_materials].map((m) => (
            <option key={m.id} value={m.id}>
              {m.name}
            </option>
          ))}
        </select>
        <span />
      </div>
      <div class="field-grid" style={{ marginTop: '8px' }}>
        <label class="plain">Suppressed</label>
        <label class="check">
          <input type="checkbox" checked={node.suppressed} onChange={(e) => store.transact(`Toggle ${id}`, (d) => (nodeIn(d, id).suppressed = (e.target as HTMLInputElement).checked))} />
          <span>Leave out of the model</span>
        </label>
      </div>
    </div>
  );
}

export function addModifier(target: string, type: Modifier['type']) {
  store.transact(`Add ${MODIFIER_LABEL[type].toLowerCase()}`, (d) => {
    const n = findNode(d, target);
    if (n && n.kind.type === 'group') n.kind.modifiers.push(defaultModifier(type));
  });
}

function DocumentInspector() {
  const doc = store.doc;
  const { errors: problems, warnings } = validateWithWarnings(doc);
  const mats = [...materials(), ...doc.custom_materials];
  const mat = mats.find((m) => m.id === doc.material);
  return (
    <div class="insp" data-inspector="document">
      <div class="insp-heading">
        <span class="kind">Document</span>
      </div>
      <div class="field-grid">
        <label class="plain">Material</label>
        <select class="select" value={doc.material} data-testid="material" onChange={(e) => store.transact('Set material', (d) => (d.material = (e.target as HTMLSelectElement).value))}>
          {mats.map((m) => (
            <option key={m.id} value={m.id}>
              {m.name}
            </option>
          ))}
        </select>
      </div>
      {mat && (
        <div class="field-grid" style={{ fontSize: '12px' }}>
          <label class="plain">Young’s modulus</label>
          <span class="num">{mat.e.toLocaleString()} MPa</span>
          <label class="plain">Poisson’s ratio</label>
          <span class="num">{mat.nu}</span>
          <label class="plain">Yield strength</label>
          <span class="num">{mat.yield_strength} MPa</span>
          <label class="plain">Density</label>
          <span class="num">{mat.density} g/cm³</span>
          <label class="plain">Minimum wall</label>
          <span class="num">{mat.min_wall} mm</span>
          <label class="plain">Source</label>
          <span style={{ color: 'var(--text-3)' }}>{mat.source}</span>
        </div>
      )}
      {problems.length > 0 && (
        <div class="error-box">
          {problems.map((p) => (
            <div key={p}>{p}</div>
          ))}
        </div>
      )}
      {warnings.length > 0 && (
        <div class="hint" data-testid="material-warnings" style={{ marginTop: '8px' }}>
          {warnings.map((w) => (
            <div key={w}>{w}</div>
          ))}
        </div>
      )}
      <div class="field-grid" style={{ marginTop: '8px' }}>
        <label class="plain">Element size</label>
        <Seg<'auto' | 'manual'>
          value={doc.solve.size === null ? 'auto' : 'manual'}
          options={[
            { value: 'auto', label: 'Auto' },
            { value: 'manual', label: 'Manual' },
          ]}
          onChange={(v) =>
            store.transact('Set element size', (d) => {
              d.solve.size = v === 'manual' ? 4 : null;
            })
          }
          testId="solve-mode"
        />
        {doc.solve.size !== null && (
          <NumRow
            label="Size"
            unit="mm"
            min={0.5}
            step={0.5}
            get={(d) => d.solve.size ?? 4}
            set={(d, v) => (d.solve.size = v)}
            undoLabel="Set element size"
            testId="solve-size"
            title="Target element size for the refined pass"
          />
        )}
        <label class="plain">Modes</label>
        <Seg<'off' | 'on'>
          value={doc.solve.modes != null ? 'on' : 'off'}
          options={[
            { value: 'off', label: 'Off' },
            { value: 'on', label: 'On', title: 'Also solve the first mode frequencies on the same mesh' },
          ]}
          onChange={(v) =>
            store.transact('Set modes', (d) => {
              d.solve.modes = v === 'on' ? 3 : null;
            })
          }
          testId="solve-modes"
        />
        {doc.solve.modes != null && (
          <NumRow
            label="Count"
            min={1}
            step={1}
            integer
            get={(d) => d.solve.modes ?? 3}
            set={(d, v) => (d.solve.modes = Math.max(1, Math.round(v)))}
            undoLabel="Set modes count"
            testId="solve-modes-count"
            title="First n natural frequencies; the Seg above skips the modal run"
          />
        )}
      </div>
      <div class="hint" style={{ marginTop: '8px' }}>
        Select a shape in the tree or the viewport to edit it.
      </div>
    </div>
  );
}

/** Inspectors for non-node selections, contributed by later lenses. */
export const inspectors: Record<string, (id: string) => JSX.Element | null> = {};

export function Inspector() {
  useApp();
  const s = store.selection;
  let body: JSX.Element | null = null;
  if (s?.kind === 'node') {
    const n = findNode(store.doc, s.id);
    if (n) body = <NodeInspector node={n} />;
  } else if (s && inspectors[s.kind]) {
    body = inspectors[s.kind](s.id);
  }
  return body ?? <DocumentInspector />;
}
