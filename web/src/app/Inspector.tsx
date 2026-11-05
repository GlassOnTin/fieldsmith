// Inspector: exact values for whatever is selected (or the document when nothing is).
import type { JSX } from 'preact';
import { useEffect, useState } from 'preact/hooks';
import { store } from '../doc/store';
import { containerOf, findNode, kindLabel, walk } from '../doc/types';
import type { Document, Face, Modifier, Node, NodeKind, Op } from '../doc/types';
import { materialLibrary, validateWithWarnings } from '../doc/wasm';
import {
  MODIFIER_LABEL,
  defaultModifier,
  findCompanionFixingNode,
  groupNode,
  inheritedColor,
  nestExistingNodeWithHost,
  positionNodeOnHostSurface,
  promoteNodeOutOfGroup,
  removeNodeAndCompanion,
  renameId,
  resolveHostSurfaceFrame,
  ungroupNode,
} from '../doc/ops';
import { applyAddr, drivenExpr, parseAddr } from '../doc/param';
import { PART_COLORS } from '../doc/types';
import {
  CONTINUUM_PHASE_META,
  MULTIPHYSICS_CONTINUUM_MATERIALS,
  ensureContinuumMaterialInDoc,
  evaluateDocumentFieldEquations,
  getContinuumMaterialSpec,
  type ContinuumPhaseId,
} from '../doc/continuum';
import { Fragment } from 'preact';
import { NumRow, NumberInput, Seg, VecRow, fmt } from './fields';
import { Icon } from './icons';
import { ui, useApp } from './ui';
import { SketchCanvas } from './Sketch';
import { openSketchWorkbenchModal } from './SketchWorkbench';
import { openFixingsCatalogueModal } from './FixingsModal';
import {
  FASTENER_MATERIAL_GROUPS,
  inspectFixingGroupStack,
  reconfigureExistingBoltStack,
  type AssociatedNutKind,
} from '../doc/fixings-catalog';

let library: ReturnType<typeof materialLibrary> | null = null;
export function materials() {
  if (!library) library = materialLibrary();
  return library;
}

function ContinuumSelectOptions({ doc }: { doc: Document }) {
  const solidLib = materials();
  const mpIds = new Set(MULTIPHYSICS_CONTINUUM_MATERIALS.map((m) => m.id));
  const extraCustom = (doc.custom_materials ?? []).filter(
    (m) => !mpIds.has(m.id) && !solidLib.some((s) => s.id === m.id),
  );
  const byPhase = (phase: ContinuumPhaseId) =>
    MULTIPHYSICS_CONTINUUM_MATERIALS.filter((m) => m.phase === phase);

  return (
    <>
      <optgroup label="Solid Structural & Thermal (Hooke + Fourier)">
        {solidLib.map((m) => (
          <option key={m.id} value={m.id}>
            {m.name}
          </option>
        ))}
        {byPhase('solid_elastic').map((m) => (
          <option key={m.id} value={m.id}>
            {m.name}
          </option>
        ))}
      </optgroup>
      <optgroup label="Viscous Melts & Glasses (VFT Extensional Sheet)">
        {byPhase('viscous_melt').map((m) => (
          <option key={m.id} value={m.id}>
            {m.name}
          </option>
        ))}
      </optgroup>
      <optgroup label="Liquid Metals & MHD Fluids (Navier–Stokes VMS)">
        {byPhase('liquid_metal').map((m) => (
          <option key={m.id} value={m.id}>
            {m.name}
          </option>
        ))}
      </optgroup>
      <optgroup label="Phase-Change & Pyrolyzing Fuels (Stefan + Darcy)">
        {byPhase('phase_change_fuel').map((m) => (
          <option key={m.id} value={m.id}>
            {m.name}
          </option>
        ))}
      </optgroup>
      <optgroup label="Optical Refractive Media (Snell / Fresnel)">
        {byPhase('optical_medium').map((m) => (
          <option key={m.id} value={m.id}>
            {m.name}
          </option>
        ))}
      </optgroup>
      <optgroup label="Buoyant Plumes, Sprays & Reactive Gases">
        {byPhase('reactive_gas').map((m) => (
          <option key={m.id} value={m.id}>
            {m.name}
          </option>
        ))}
      </optgroup>
      {extraCustom.length > 0 && (
        <optgroup label="Custom Document Materials">
          {extraCustom.map((m) => (
            <option key={m.id} value={m.id}>
              {m.name}
            </option>
          ))}
        </optgroup>
      )}
    </>
  );
}

function ContinuumConstitutiveCard({
  doc,
  matId,
  testId,
}: {
  doc: Document;
  matId: string;
  testId: string;
}) {
  const spec = getContinuumMaterialSpec(doc, matId);
  const phaseMeta = CONTINUUM_PHASE_META[spec.phase];
  const accentText = `color-mix(in srgb, ${spec.badgeColor} 65%, var(--text))`;
  return (
    <div
      class="card"
      data-testid={testId}
      style={{
        marginTop: '8px',
        padding: '9px 10px',
        border: `1px solid color-mix(in srgb, ${spec.badgeColor} 38%, var(--line-strong))`,
        background: 'var(--panel-2)',
        borderRadius: '6px',
      }}
    >
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '6px', flexWrap: 'wrap', marginBottom: '6px' }}>
        <span style={{ fontSize: '11px', fontWeight: 700, color: 'var(--text)' }}>Continuum Phase</span>
        <span
          data-testid={`${testId}-phase`}
          style={{
            fontSize: '10px',
            fontWeight: 700,
            padding: '2px 7px',
            borderRadius: '5px',
            background: `color-mix(in srgb, ${spec.badgeColor} 14%, var(--panel))`,
            color: accentText,
            border: `1px solid color-mix(in srgb, ${spec.badgeColor} 40%, var(--line-strong))`,
          }}
        >
          {spec.phaseLabel}
        </span>
      </div>
      <div style={{ fontSize: '11px', color: 'var(--text-2)', marginBottom: '7px', lineHeight: 1.4 }}>
        {phaseMeta.summary}
      </div>
      <div
        style={{
          fontFamily: 'var(--mono)',
          fontSize: '10.5px',
          lineHeight: 1.45,
          padding: '6px 8px',
          background: 'var(--panel)',
          border: '1px solid var(--line)',
          borderLeft: `3px solid ${spec.badgeColor}`,
          borderRadius: '4px',
          color: 'var(--text)',
          marginBottom: '7px',
          whiteSpace: 'pre-wrap',
          wordBreak: 'break-word',
        }}
        title="Active constitutive relation for this volume"
      >
        {spec.constitutiveEquation}
      </div>
      <div style={{ display: 'grid', gridTemplateColumns: 'auto 1fr auto', gap: '4px 8px', fontSize: '11px', alignItems: 'baseline' }}>
        {spec.parameters.map((p) => (
          <Fragment key={p.symbol}>
            <span class="num" style={{ color: accentText, fontWeight: 700 }}>
              {p.symbol}
            </span>
            <span style={{ color: 'var(--text-2)' }}>{p.label}</span>
            <span class="num" style={{ textAlign: 'right', color: 'var(--text)', fontWeight: 600 }}>
              {p.value} {p.unit !== '—' ? p.unit : ''}
            </span>
          </Fragment>
        ))}
      </div>
      <div style={{ marginTop: '8px', display: 'flex', justifyContent: 'flex-end' }}>
        <button
          type="button"
          class="btn ghost small"
          data-testid={`${testId}-open-theory`}
          onClick={() => store.setLens('theory')}
          style={{ fontSize: '10.5px', padding: '2px 8px', color: 'var(--accent)', fontWeight: 600 }}
        >
          Applied Math in Theory (6) →
        </button>
      </div>
    </div>
  );
}
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
  // The refusal hint when a .fsm let's expression writes the attribute: the field is
  // display-only until the let is swept.
  const drv = (attr: string, comp: number | null = null) => drivenExpr(store.doc, id, attr, comp);
  switch (node.kind.type) {
    case 'box':
      return (
        <>
          <VecRow label="Size" unit="mm" min={0.01} get={(d) => kindIn(d, id, 'box').size} set={(d, v) => (kindIn(d, id, 'box').size = v)} undoLabel={u} testId="size" driven={(i) => drv('size', i)} />
          <NumRow label="Corner radius" unit="mm" min={0} step={0.5} get={(d) => kindIn(d, id, 'box').round} set={(d, v) => (kindIn(d, id, 'box').round = v)} undoLabel={u} testId="round" driven={drv('round')} />
        </>
      );
    case 'cylinder':
      return (
        <>
          <NumRow label="Diameter" unit="mm" min={0.01} step={0.5} get={(d) => kindIn(d, id, 'cylinder').radius * 2} set={(d, v) => (kindIn(d, id, 'cylinder').radius = v / 2)} undoLabel={u} testId="diameter" driven={drv('radius')} />
          <NumRow label="Height" unit="mm" min={0.01} get={(d) => kindIn(d, id, 'cylinder').height} set={(d, v) => (kindIn(d, id, 'cylinder').height = v)} undoLabel={u} testId="height" driven={drv('height')} />
          <NumRow label="Edge radius" unit="mm" min={0} step={0.5} get={(d) => kindIn(d, id, 'cylinder').round} set={(d, v) => (kindIn(d, id, 'cylinder').round = v)} undoLabel={u} driven={drv('round')} />
        </>
      );
    case 'sphere':
      return <NumRow label="Diameter" unit="mm" min={0.01} get={(d) => kindIn(d, id, 'sphere').radius * 2} set={(d, v) => (kindIn(d, id, 'sphere').radius = v / 2)} undoLabel={u} testId="diameter" driven={drv('radius')} />;
    case 'capsule':
      return (
        <>
          <NumRow label="Diameter" unit="mm" min={0.01} get={(d) => kindIn(d, id, 'capsule').radius * 2} set={(d, v) => (kindIn(d, id, 'capsule').radius = v / 2)} undoLabel={u} driven={drv('radius')} />
          <NumRow label="Length" unit="mm" min={0} title="Straight length between the hemispherical ends" get={(d) => kindIn(d, id, 'capsule').length} set={(d, v) => (kindIn(d, id, 'capsule').length = v)} undoLabel={u} driven={drv('length')} />
        </>
      );
    case 'torus':
      return (
        <>
          <NumRow label="Ring radius" unit="mm" min={0.01} get={(d) => kindIn(d, id, 'torus').major} set={(d, v) => (kindIn(d, id, 'torus').major = v)} undoLabel={u} driven={drv('major')} />
          <NumRow label="Tube radius" unit="mm" min={0.01} step={0.5} get={(d) => kindIn(d, id, 'torus').minor} set={(d, v) => (kindIn(d, id, 'torus').minor = v)} undoLabel={u} driven={drv('minor')} />
        </>
      );
    case 'extrude':
      return (
        <>
          <NumRow label="Height" unit="mm" min={0.01} get={(d) => kindIn(d, id, 'extrude').height} set={(d, v) => (kindIn(d, id, 'extrude').height = v)} undoLabel={u} driven={drv('height')} />
        </>
      );
    case 'revolve':
      return (
        <>
          <NumRow label="Sweep angle" unit="°" min={1} max={360} step={15} get={(d) => kindIn(d, id, 'revolve').angle} set={(d, v) => (kindIn(d, id, 'revolve').angle = v)} undoLabel={u} driven={drv('angle')} />
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
  if (node.kind.type !== 'extrude' && node.kind.type !== 'revolve') return null;
  const id = node.id;
  const pts = node.kind.profile;
  const edit = (label: string, f: (p: { x: number; y: number; bulge: number }[]) => void) =>
    store.transact(label, (d) => {
      const k = nodeIn(d, id).kind;
      if (k.type === 'extrude' || k.type === 'revolve') f(k.profile);
    });
  return (
    <div class="card">
      <div class="card-head">
        <span>Profile</span>
        <div style={{ display: 'flex', gap: '4px' }}>
          <button
            class="btn ghost small"
            title="Open Parametric 2D Constraint Solver & Surface-Feature Workbench"
            data-testid="inspector-open-sketch-workbench"
            onClick={() => openSketchWorkbenchModal()}
            style={{ display: 'inline-flex', alignItems: 'center', gap: '4px' }}
          >
            <Icon.sketcher /> Constraints…
          </button>
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
      </div>
      <div style={{ padding: '6px 8px 2px' }}>
        <SketchCanvas id={id} />
        <div class="hint" style={{ marginTop: '4px' }}>
          Sketch in the profile's own X-Y. Click an edge to add a point there; drag
          a dot to move it. The first dot is where the closed profile starts.
        </div>
      </div>
      <div style={{ padding: '4px 8px 6px' }}>
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

function BoltHardwareAndForcesPanel({ node }: { node: Node }) {
  const stack = inspectFixingGroupStack(store.doc, node.id);
  if (!stack) return null;

  const { sizeSpec, includeWasher, associatedNut, fastenerMaterial, envelope, hwGroupId } = stack;
  const hf = envelope.holdableForces;

  const applyStackChange = (
    nextWasher: boolean,
    nextNut: AssociatedNutKind,
    nextMat = fastenerMaterial,
  ) => {
    store.transact(
      `Update ${hwGroupId} hardware stack (washer=${nextWasher}, nut=${nextNut}, mat=${nextMat})`,
      (d) => {
        reconfigureExistingBoltStack(d, hwGroupId, {
          includeWasher: nextWasher,
          associatedNut: nextNut,
          fastenerMaterial: nextMat,
        });
      },
    );
    ui.notify(
      `Updated ${hwGroupId}: ${nextWasher ? 'Washer ON' : 'Washer OFF'}, Nut = ${nextNut}, Mat = ${nextMat}`,
      'info',
    );
  };

  return (
    <div
      data-testid="inspector-fixing-hardware-card"
      style={{
        marginTop: '8px',
        padding: '9px 10px',
        borderRadius: '8px',
        background: 'rgba(56, 189, 248, 0.08)',
        border: '1.5px solid rgba(56, 189, 248, 0.45)',
        display: 'flex',
        flexDirection: 'column',
        gap: '7px',
        fontSize: '11.5px',
      }}
    >
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          gap: '6px',
          flexWrap: 'wrap',
        }}
      >
        <span style={{ fontWeight: 700, color: 'var(--accent)' }}>
          🔩 Bolt Stack ({sizeSpec.name} × {stack.engagementMm} mm) &amp; Holdable Forces
        </span>
        <span style={{ fontSize: '10px', fontFamily: 'var(--mono)', color: 'var(--text-2)' }}>
          K = {envelope.nutFactorK.toFixed(3)} · A_brg = {envelope.bearingAreaMm2.toFixed(1)} mm²
        </span>
      </div>

      {/* Washer & Nut Controls */}
      <div
        style={{
          display: 'flex',
          flexDirection: 'column',
          gap: '6px',
          padding: '6px 8px',
          borderRadius: '6px',
          background: 'rgba(56, 189, 248, 0.08)',
          border: '1px solid rgba(56, 189, 248, 0.28)',
        }}
      >
        <label
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: '7px',
            cursor: 'pointer',
            fontWeight: 600,
          }}
        >
          <input
            type="checkbox"
            checked={includeWasher}
            data-testid="inspector-fixing-washer-toggle"
            style={{ width: '14px', height: '14px', margin: 0, flex: 'none' }}
            onChange={(e) =>
              applyStackChange((e.target as HTMLInputElement).checked, associatedNut)
            }
          />
          <span>
            ⭕ Include Bearing Washer (⌀{sizeSpec.washer_od.toFixed(1)}×
            {sizeSpec.washer_t.toFixed(1)} mm)
          </span>
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
          <span style={{ fontSize: '10.5px', fontWeight: 600, color: 'var(--text-2)' }}>
            ⬡ Associated Mated Nut
          </span>
          <select
            class="select"
            data-testid="inspector-fixing-nut-select"
            value={associatedNut}
            onChange={(e) =>
              applyStackChange(
                includeWasher,
                (e.target as HTMLSelectElement).value as AssociatedNutKind,
              )
            }
          >
            <option value="none">None (Tapped into Host)</option>
            <option value="hex_nut">
              ⬡ Hex Nut (ISO 4032 — s={sizeSpec.hex_flats_s} mm, m={sizeSpec.nut_h} mm)
            </option>
            <option value="nyloc_nut">
              🔒 Nyloc Lock Nut (ISO 10511 — +Prevailing Torque)
            </option>
          </select>
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
          <span style={{ fontSize: '10.5px', fontWeight: 600, color: 'var(--text-2)' }}>
            🔩 Fastener Hardware Material
          </span>
          <select
            class="select"
            data-testid="inspector-fixing-material-select"
            value={fastenerMaterial}
            onChange={(e) =>
              applyStackChange(
                includeWasher,
                associatedNut,
                (e.target as HTMLSelectElement).value,
              )
            }
          >
            {FASTENER_MATERIAL_GROUPS.map((grp) => (
              <optgroup key={grp.group} label={grp.group}>
                {grp.options.map((opt) => (
                  <option key={opt.id} value={opt.id}>
                    {opt.label}
                  </option>
                ))}
              </optgroup>
            ))}
          </select>
        </label>
      </div>

      {/* Torque Requirements */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: '1fr 1fr',
          gap: '4px 8px',
          fontFamily: 'var(--mono)',
          fontSize: '10.5px',
          padding: '5px 7px',
          borderRadius: '6px',
          background: 'rgba(16, 185, 129, 0.08)',
          border: '1px solid rgba(16, 185, 129, 0.3)',
        }}
      >
        <div>
          T_rec: <strong style={{ color: '#34d399' }}>{envelope.recommendedTorqueNm.toFixed(2)} N·m</strong>
        </div>
        <div>
          Window: <strong>[{envelope.minTorqueNm.toFixed(2)}, {envelope.maxTorqueNm.toFixed(2)}] N·m</strong>
        </div>
        <div>
          Bearing σ_brg: <strong>{envelope.bearingPressureAtRecMpa.toFixed(1)} MPa</strong>
        </div>
        <div>
          Creep Kept: <strong>{(envelope.creepRetentionFactor * 100).toFixed(0)}%</strong>
        </div>
      </div>

      {/* Holdable Forces Readout */}
      <div
        data-testid="inspector-fixing-forces-grid"
        style={{
          display: 'grid',
          gridTemplateColumns: '1fr 1fr',
          gap: '3px 8px',
          fontFamily: 'var(--mono)',
          fontSize: '10.5px',
        }}
      >
        <div title="Axial clamping preload force at T_rec">
          Preload F_0:{' '}
          <strong>
            {hf.axialPreloadAtRecN.toFixed(0)} N ({(hf.axialPreloadAtRecN / 9.80665).toFixed(1)} kgf)
          </strong>
        </div>
        <div title="Residual clamp force after creep relaxation">
          After Creep:{' '}
          <strong>
            {hf.residualClampAfterCreepN.toFixed(0)} N ({(hf.residualClampAfterCreepN / 9.80665).toFixed(1)} kgf)
          </strong>
        </div>
        <div title="Max external axial pull force before joint separates (gaps)">
          Axial No-Gap F_sep:{' '}
          <strong style={{ color: '#38bdf8' }}>
            {hf.maxAxialLoadBeforeGappingN.toFixed(0)} N ({(hf.maxAxialLoadBeforeGappingN / 9.80665).toFixed(1)} kgf)
          </strong>
        </div>
        <div title="Max transverse shear force held by friction before slip">
          Friction Grip F_slip:{' '}
          <strong style={{ color: '#34d399' }}>
            {hf.maxTransverseFrictionSlipN.toFixed(0)} N ({(hf.maxTransverseFrictionSlipN / 9.80665).toFixed(1)} kgf)
          </strong>
        </div>
        <div title="Ultimate thread pull-out / stripping capacity">
          Thread Pull-Out:{' '}
          <strong>
            {hf.maxThreadPulloutN.toFixed(0)} N ({(hf.maxThreadPulloutN / 9.80665).toFixed(1)} kgf)
          </strong>
        </div>
        <div title="Underhead / washer surface crush capacity">
          Bearing Crush:{' '}
          <strong>
            {hf.maxUnderheadBearingN.toFixed(0)} N ({(hf.maxUnderheadBearingN / 9.80665).toFixed(1)} kgf)
          </strong>
        </div>
        <div title="Bolt shank pin shear capacity">
          Shank Shear:{' '}
          <strong>
            {hf.maxShankShearPinN.toFixed(0)} N ({(hf.maxShankShearPinN / 9.80665).toFixed(1)} kgf)
          </strong>
        </div>
        <div title="Safe continuous working axial / shear loads">
          Safe Work (Ax/Sh):{' '}
          <strong style={{ color: '#fbbf24' }}>
            {hf.safeWorkingAxialLoadN.toFixed(0)} / {hf.safeWorkingShearLoadN.toFixed(0)} N
          </strong>
        </div>
      </div>
    </div>
  );
}

function SurfaceMateAndTreePanel({ node }: { node: Node }) {
  const doc = store.doc;
  const id = node.id;
  const companion = findCompanionFixingNode(doc, id);
  const cont = containerOf(doc, id);
  const parentGroup = cont?.parent ?? null;

  // Collect candidate host parts (excluding `node` itself, its companion, and its descendants)
  const excludedIds = new Set<string>([id]);
  if (companion) excludedIds.add(companion.id);
  walk([node], (n) => excludedIds.add(n.id));
  if (companion) walk([companion], (n) => excludedIds.add(n.id));

  const hostCandidates: Node[] = [];
  const collectHosts = (nodes: Node[]) => {
    for (const n of nodes) {
      if (excludedIds.has(n.id) || n.suppressed || n.op !== 'add') continue;
      if (n.kind.type === 'group') {
        collectHosts(n.kind.children);
      } else {
        hostCandidates.push(n);
      }
    }
  };
  collectHosts(doc.root);

  const [hostId, setHostId] = useState<string>(() => hostCandidates[0]?.id ?? '');
  const [face, setFace] = useState<Face>('top');
  const [mode, setMode] = useState<'edge' | 'center'>('edge');
  const [seating, setSeating] = useState<'flush' | 'centered' | 'recessed'>(() =>
    companion ? 'centered' : 'flush',
  );
  const [bindLets, setBindLets] = useState<boolean>(true);

  const activeHostId =
    (hostId && hostCandidates.some((h) => h.id === hostId) ? hostId : undefined) ??
    hostCandidates[0]?.id ??
    '';

  const frame = activeHostId ? resolveHostSurfaceFrame(doc, activeHostId, face) : null;
  const currentUV = frame
    ? frame.uvFromPoint(node.transform.translate)
    : { u: 0, v: 0, dx: 10, dy: 10, normalOffset: 0 };

  const applyPosition = (
    nextDx: number,
    nextDy: number,
    nextU: number,
    nextV: number,
    nextSeating = seating,
    nextFace = face,
    nextHostId = activeHostId,
  ) => {
    if (!nextHostId) return;
    store.transact(`Position ${id} on ${nextHostId}.${nextFace}`, (d) => {
      positionNodeOnHostSurface(d, id, {
        hostId: nextHostId,
        face: nextFace,
        mode,
        dx: nextDx,
        dy: nextDy,
        u: nextU,
        v: nextV,
        seating: nextSeating,
        bindLets,
        associateInTree: false,
      });
    });
  };

  return (
    <div
      class="card"
      data-testid="surface-mate-panel"
      style={{
        marginTop: '8px',
        padding: '8px 10px',
        background: 'var(--panel-2)',
        border: '1px solid var(--line-strong)',
        borderRadius: '6px',
      }}
    >
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          gap: '6px',
          marginBottom: '6px',
        }}
      >
        <span style={{ fontSize: '11px', fontWeight: 700, color: 'var(--accent)' }}>
          📐 Surface Mate &amp; Object Tree
        </span>
        {companion && (
          <span
            style={{
              fontSize: '10px',
              padding: '1px 6px',
              borderRadius: '4px',
              background: 'rgba(56, 189, 248, 0.14)',
              color: 'var(--accent)',
              fontFamily: 'var(--mono)',
            }}
            title="Paired fixing assembly: moving this node moves its companion bore/hardware group in lockstep"
          >
            🔗 {companion.id}
          </span>
        )}
      </div>

      {/* Tree Hierarchy Status & Actions */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          gap: '6px',
          flexWrap: 'wrap',
          fontSize: '11px',
          marginBottom: '8px',
          paddingBottom: '6px',
          borderBottom: '1px solid var(--line)',
        }}
      >
        <span style={{ color: 'var(--text-2)' }}>
          Tree:{' '}
          <strong style={{ color: 'var(--text)' }}>
            {parentGroup ? `📁 ${parentGroup.id}` : 'Root'}
          </strong>
        </span>
        <div style={{ display: 'flex', gap: '4px', flexWrap: 'wrap' }}>
          {node.kind.type === 'group' && (
            <button
              type="button"
              class="btn small"
              data-testid="tree-ungroup-keep-children-btn"
              title={`Dissolve group ${id} and keep all ${node.kind.children.length} child object(s) in place`}
              onClick={() => {
                let kept: Node[] = [];
                store.transact(`Ungroup ${id}`, (d) => {
                  kept = ungroupNode(d, id);
                });
                if (kept.length > 0) {
                  store.select({ kind: 'node', id: kept[0].id });
                  ui.notify(`Ungrouped ${id} (${kept.length} child object${kept.length === 1 ? '' : 's'} kept)`, 'info');
                } else {
                  store.select(null);
                }
              }}
              style={{ fontSize: '10px', padding: '2px 6px' }}
            >
              📂 Ungroup (Keep {node.kind.children.length})
            </button>
          )}
          {activeHostId && (
            <button
              type="button"
              class="btn small"
              data-testid="tree-associate-host-btn"
              title={`Nest ${id} inside ${activeHostId}'s assembly group in the Object Tree`}
              onClick={() => {
                store.transact(`Associate ${id} with ${activeHostId}`, (d) => {
                  nestExistingNodeWithHost(d, id, activeHostId);
                });
                ui.notify(`Associated ${id} under ${activeHostId} assembly group`, 'info');
              }}
              style={{ fontSize: '10px', padding: '2px 6px' }}
            >
              🔗 Group with {activeHostId}
            </button>
          )}
          {node.kind.type !== 'group' && !parentGroup && (
            <button
              type="button"
              class="btn ghost small"
              data-testid="tree-wrap-group-btn"
              title="Wrap this node in a new Group (Ctrl+G)"
              onClick={() => {
                let gid = '';
                store.transact(`Group ${id}`, (d) => {
                  const g = groupNode(d, id);
                  if (g) gid = g.id;
                });
                if (gid) store.select({ kind: 'node', id: gid });
              }}
              style={{ fontSize: '10px', padding: '2px 6px' }}
            >
              📁 Wrap Group
            </button>
          )}
          {parentGroup && (
            <button
              type="button"
              class="btn ghost small"
              data-testid="tree-promote-btn"
              title={`Move ${id} out of group ${parentGroup.id}`}
              onClick={() =>
                store.transact(`Promote ${id}`, (d) => {
                  promoteNodeOutOfGroup(d, id);
                })
              }
              style={{ fontSize: '10px', padding: '2px 6px' }}
            >
              ⬆ Remove from {parentGroup.id}
            </button>
          )}
          <button
            type="button"
            class="btn ghost small"
            data-testid="tree-delete-node-btn"
            title={
              node.kind.type === 'group'
                ? `Delete group ${id} and all ${node.kind.children.length} child object(s)${companion ? ` (+ companion ${companion.id})` : ''}`
                : `Delete ${id}`
            }
            onClick={() => {
              store.transact(`Delete ${id}`, (d) => {
                removeNodeAndCompanion(d, id, Boolean(companion));
              });
              store.select(null);
              ui.notify(
                node.kind.type === 'group'
                  ? `Deleted group ${id} and its child objects`
                  : `Deleted ${id}`,
                'info',
              );
            }}
            style={{ fontSize: '10px', padding: '2px 6px', color: 'var(--bad, #ef4444)' }}
          >
            🗑 {node.kind.type === 'group' ? `Delete Group & All (${node.kind.children.length})` : 'Delete'}
          </button>
        </div>
      </div>

      {hostCandidates.length === 0 ? (
        <div class="hint" style={{ padding: 0 }}>
          Add another shape to mate or dimension this part onto a host surface.
        </div>
      ) : (
        <div style={{ display: 'flex', flexDirection: 'column', gap: '6px', fontSize: '11px' }}>
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '6px' }}>
            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
              <span style={{ color: 'var(--text-2)', fontWeight: 600 }}>Host Part</span>
              <select
                class="select"
                data-testid="surface-mate-host"
                value={activeHostId}
                onChange={(e) => setHostId((e.target as HTMLSelectElement).value)}
              >
                {hostCandidates.map((h) => (
                  <option key={h.id} value={h.id}>
                    {h.id} ({h.kind.type})
                  </option>
                ))}
              </select>
            </label>
            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
              <span style={{ color: 'var(--text-2)', fontWeight: 600 }}>Target Surface</span>
              <select
                class="select"
                data-testid="surface-mate-face"
                value={face}
                onChange={(e) => {
                  const nextF = (e.target as HTMLSelectElement).value as Face;
                  setFace(nextF);
                  if (frame) {
                    const nf = resolveHostSurfaceFrame(doc, activeHostId, nextF);
                    applyPosition(
                      0.5 * nf.faceWidth,
                      0.5 * nf.faceHeight,
                      0,
                      0,
                      seating,
                      nextF,
                      activeHostId,
                    );
                  }
                }}
              >
                <option value="top">Top (+Z)</option>
                <option value="bottom">Bottom (−Z)</option>
                <option value="px">Right (+X)</option>
                <option value="nx">Left (−X)</option>
                <option value="py">Back (+Y)</option>
                <option value="ny">Front (−Y)</option>
              </select>
            </label>
          </div>

          <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '4px' }}>
            <button
              type="button"
              class={`btn small ${mode === 'edge' ? 'primary' : ''}`}
              data-testid="surface-mate-mode-edge"
              onClick={() => setMode('edge')}
              style={{ fontSize: '10.5px', padding: '3px 6px' }}
            >
              📏 Edge Distances (dx, dy)
            </button>
            <button
              type="button"
              class={`btn small ${mode === 'center' ? 'primary' : ''}`}
              data-testid="surface-mate-mode-center"
              onClick={() => setMode('center')}
              style={{ fontSize: '10.5px', padding: '3px 6px' }}
            >
              ⊕ Centre Offset (u, v)
            </button>
          </div>

          {frame && mode === 'edge' && (
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '6px' }}>
              <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
                <span style={{ color: 'var(--text-2)', fontWeight: 600 }}>
                  dx from −{frame.uLabel} edge (mm)
                </span>
                <NumberInput
                  value={currentUV.dx}
                  step={1}
                  testId="surface-mate-dx"
                  onChange={(v) =>
                    applyPosition(v, currentUV.dy, v - 0.5 * frame.faceWidth, currentUV.v)
                  }
                />
              </label>
              <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
                <span style={{ color: 'var(--text-2)', fontWeight: 600 }}>
                  dy from −{frame.vLabel} edge (mm)
                </span>
                <NumberInput
                  value={currentUV.dy}
                  step={1}
                  testId="surface-mate-dy"
                  onChange={(v) =>
                    applyPosition(currentUV.dx, v, currentUV.u, v - 0.5 * frame.faceHeight)
                  }
                />
              </label>
            </div>
          )}

          {frame && mode === 'center' && (
            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '6px' }}>
              <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
                <span style={{ color: 'var(--text-2)', fontWeight: 600 }}>
                  u along {frame.uLabel} (mm)
                </span>
                <NumberInput
                  value={currentUV.u}
                  step={1}
                  testId="surface-mate-u"
                  onChange={(v) =>
                    applyPosition(0.5 * frame.faceWidth + v, currentUV.dy, v, currentUV.v)
                  }
                />
              </label>
              <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
                <span style={{ color: 'var(--text-2)', fontWeight: 600 }}>
                  v along {frame.vLabel} (mm)
                </span>
                <NumberInput
                  value={currentUV.v}
                  step={1}
                  testId="surface-mate-v"
                  onChange={(v) =>
                    applyPosition(currentUV.dx, 0.5 * frame.faceHeight + v, currentUV.u, v)
                  }
                />
              </label>
            </div>
          )}

          {!companion && (
            <div style={{ display: 'grid', gridTemplateColumns: 'auto 1fr', gap: '6px', alignItems: 'center' }}>
              <span style={{ color: 'var(--text-2)', fontWeight: 600 }}>Seating</span>
              <Seg<'flush' | 'centered' | 'recessed'>
                value={seating}
                options={[
                  { value: 'flush', label: 'Flush', title: 'Base sits flush on host face' },
                  { value: 'centered', label: 'Centred', title: 'Origin lies on host face' },
                  { value: 'recessed', label: 'Recessed', title: 'Sunk inside host face' },
                ]}
                onChange={(s) => {
                  setSeating(s);
                  applyPosition(currentUV.dx, currentUV.dy, currentUV.u, currentUV.v, s);
                }}
              />
            </div>
          )}

          <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '6px' }}>
            <label style={{ display: 'inline-flex', alignItems: 'center', gap: '5px', cursor: 'pointer' }}>
              <input
                type="checkbox"
                checked={bindLets}
                data-testid="surface-mate-bind-lets"
                onChange={(e) => setBindLets((e.target as HTMLInputElement).checked)}
              />
              <span>Bind <code>let</code> params</span>
            </label>
            {frame && (
              <button
                type="button"
                class="btn small primary"
                data-testid="surface-mate-apply-btn"
                onClick={() =>
                  applyPosition(currentUV.dx, currentUV.dy, currentUV.u, currentUV.v)
                }
                style={{ fontSize: '10.5px', padding: '3px 8px' }}
              >
                Snap to {activeHostId}.{face}
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  );
}

function NodeInspector({ node }: { node: Node }) {
  const id = node.id;
  const u = `Edit ${id}`;
  const inherited = node.color ? undefined : inheritedColor(store.doc, id);
  return (
    <div class="insp" data-inspector="node">
      <div class="insp-heading" style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '6px', flexWrap: 'wrap' }}>
        <span class="kind">{kindLabel(node.kind)}</span>
        <div style={{ display: 'flex', gap: '4px', alignItems: 'center', flexWrap: 'wrap' }}>
          {node.op === 'add' && node.kind.type !== 'group' && !findCompanionFixingNode(store.doc, id) && (
            <button
              type="button"
              class="btn small"
              data-testid="inspector-attach-bolt-btn"
              title={`Add a bolt or fixing attached to ${id} (choose target surface & edge dimensions)`}
              onClick={() => openFixingsCatalogueModal('hex_cap_bolt', id, 'top')}
              style={{
                fontSize: '10.5px',
                padding: '2px 7px',
                fontWeight: 600,
                background: 'rgba(56, 189, 248, 0.12)',
                borderColor: 'rgba(56, 189, 248, 0.4)',
              }}
            >
              🔩 Attach Bolt to {id}…
            </button>
          )}
          {node.kind.type === 'group' && (
            <button
              type="button"
              class="btn ghost small"
              data-testid="inspector-ungroup-btn"
              title={`Dissolve group ${id} and keep all ${node.kind.children.length} child object(s)`}
              onClick={() => {
                let kept: Node[] = [];
                store.transact(`Ungroup ${id}`, (d) => {
                  kept = ungroupNode(d, id);
                });
                if (kept.length > 0) {
                  store.select({ kind: 'node', id: kept[0].id });
                  ui.notify(`Ungrouped ${id} (${kept.length} child object${kept.length === 1 ? '' : 's'} kept)`, 'info');
                } else {
                  store.select(null);
                }
              }}
              style={{ fontSize: '10.5px', padding: '2px 7px' }}
            >
              📂 Ungroup (Keep {node.kind.children.length})
            </button>
          )}
          <button
            type="button"
            class="btn ghost small"
            data-testid="inspector-delete-node-btn"
            title={
              node.kind.type === 'group'
                ? `Delete group ${id} and all ${node.kind.children.length} child object(s) (Del)`
                : `Delete ${id} (Del)`
            }
            onClick={() => {
              store.transact(`Delete ${id}`, (d) => {
                removeNodeAndCompanion(d, id, Boolean(findCompanionFixingNode(d, id)));
              });
              store.select(null);
            }}
            style={{ fontSize: '10.5px', padding: '2px 7px', color: 'var(--bad, #ef4444)', display: 'inline-flex', alignItems: 'center', gap: '4px' }}
          >
            <Icon.trash />
            <span>{node.kind.type === 'group' ? `Delete Group (${node.kind.children.length})` : 'Delete'}</span>
          </button>
        </div>
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
        <VecRow
          label="Position"
          unit="mm"
          get={(d) => nodeIn(d, id).transform.translate}
          set={(d, v) => {
            nodeIn(d, id).transform.translate = v;
            const comp = findCompanionFixingNode(d, id);
            if (comp) comp.transform.translate = [...v];
          }}
          undoLabel={`Move ${id}`}
          testId="position"
          driven={(i) => drivenExpr(store.doc, id, 'at', i)}
        />
        <VecRow
          label="Rotation"
          unit="°"
          step={15}
          get={(d) => nodeIn(d, id).transform.rotate}
          set={(d, v) => {
            nodeIn(d, id).transform.rotate = v;
            const comp = findCompanionFixingNode(d, id);
            if (comp) comp.transform.rotate = [...v];
          }}
          undoLabel={`Rotate ${id}`}
          testId="rotation"
          driven={(i) => drivenExpr(store.doc, id, 'rotate', i)}
        />
      </div>
      <BoltHardwareAndForcesPanel node={node} />
      <SurfaceMateAndTreePanel node={node} />
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
          title="Per-volume continuum material & constitutive model (Solid Elastic, VFT Viscous Melt, Liquid Metal MHD, Phase-Change Fuel, Optical Dielectric, or Reactive Gas)"
          onChange={(e) =>
            store.transact(`Label ${id}`, (d) => {
              const v = (e.target as HTMLSelectElement).value;
              if (v === '') {
                delete nodeIn(d, id).material;
              } else {
                ensureContinuumMaterialInDoc(d, v);
                nodeIn(d, id).material = v;
              }
            })
          }
        >
          <option value="">Document default ({store.doc.material})</option>
          <ContinuumSelectOptions doc={store.doc} />
        </select>
        <span />
        <label class="plain">Colour</label>
        <div class="chips" data-testid="node-color">
          {PART_COLORS.map((c) => (
            <button
              key={c}
              class={`swatch ${node.color === c ? 'on' : ''}`}
              style={{ background: c }}
              data-color={c}
              title={`Colour this shape ${c}`}
              onClick={() =>
                store.transact(`Colour ${id}`, (d) => {
                  const n = nodeIn(d, id);
                  if (n.color === c) delete n.color;
                  else n.color = c;
                })
              }
            />
          ))}
          <button
            class="mini swatch-clear"
            data-color=""
            title="Clear this shape's colour — it inherits the nearest group's colour above"
            onClick={() =>
              store.transact(`Colour ${id}`, (d) => {
                delete nodeIn(d, id).color;
              })
            }
          >
            ✕
          </button>
        </div>
        <span class="hint" data-testid="color-hint">
          {node.color
            ? `${node.color} — this shape`
            : inherited
              ? `${inherited.color} from ${inherited.from}`
              : 'document base colour'}
        </span>
      </div>
      <div class="field-grid" style={{ marginTop: '8px' }}>
        <label class="plain">Faces</label>
        <span class="num" style={{ color: 'var(--text-3)', fontSize: '11px' }} title="Valid face selector names for regions in Setup (2) and MCP add_region">
          {node.kind.type === 'box'
            ? 'px · nx · py · ny · pz · nz · all'
            : node.kind.type === 'cylinder' || node.kind.type === 'extrude'
              ? 'top · bottom · side · all'
              : 'all'}
        </span>
        <label class="plain">Suppressed</label>
        <label class="check">
          <input type="checkbox" checked={node.suppressed} onChange={(e) => store.transact(`Toggle ${id}`, (d) => (nodeIn(d, id).suppressed = (e.target as HTMLInputElement).checked))} />
          <span>Leave out of the model</span>
        </label>
      </div>
      <ContinuumConstitutiveCard
        doc={store.doc}
        matId={node.material || store.doc.material}
        testId="node-continuum-card"
      />
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
  ensureContinuumMaterialInDoc(doc, doc.material);
  const { errors: problems, warnings } = validateWithWarnings(doc);
  const mats = [
    ...materials(),
    ...doc.custom_materials,
    ...MULTIPHYSICS_CONTINUUM_MATERIALS.map((m) => m.fsmMaterial),
  ];
  const mat = mats.find((m) => m.id === doc.material);
  const activeEqs = evaluateDocumentFieldEquations(doc).filter((e) => e.active);
  const addNewLet = () => {
    store.transact('Add parameter', (d) => {
      const lets = d.lets ?? (d.lets = []);
      let idx = lets.length + 1;
      let name = `p${idx}`;
      while (lets.some((l) => l.name === name)) {
        idx++;
        name = `p${idx}`;
      }
      lets.push({ name, expr: '10', value: 10 });
    });
  };
  return (
    <div class="insp" data-inspector="document">
      <div class="insp-heading">
        <span class="kind">Document</span>
      </div>
      <div class="field-grid">
        <label class="plain">Material</label>
        <select
          class="select"
          value={doc.material}
          data-testid="material"
          onChange={(e) =>
            store.transact('Set material', (d) => {
              const v = (e.target as HTMLSelectElement).value;
              ensureContinuumMaterialInDoc(d, v);
              d.material = v;
            })
          }
        >
          <ContinuumSelectOptions doc={doc} />
        </select>
      </div>
      <ContinuumConstitutiveCard doc={doc} matId={doc.material} testId="doc-continuum-card" />
      {mat && (
        <>
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
          </div>
          <details class="hint" style={{ marginTop: '4px' }}>
            <summary style={{ cursor: 'pointer', userSelect: 'none' }}>Material source datasheet</summary>
            <div style={{ marginTop: '4px', color: 'var(--text-3)' }}>{mat.source}</div>
          </details>
        </>
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
      <div class="section-label" style={{ marginTop: '8px', display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
        <span>Parameters (let)</span>
        <button
          type="button"
          class="btn ghost small"
          data-testid="add-let-btn"
          title="Add a named scalar parameter (let)"
          onClick={addNewLet}
        >
          + let
        </button>
      </div>
      {(doc.lets?.length ?? 0) > 0 ? (
        <div class="field-grid" data-testid="params-panel">
          {doc.lets?.map((l) => {
            const isLiteral = /^-?\d+(\.\d+)?$/.test(l.expr.trim());
            return (
              <Fragment key={l.name}>
                <label class="plain">{l.name}</label>
                {isLiteral ? (
                  <NumberInput
                    value={l.value}
                    step={0.5}
                    testId={`let-input-${l.name}`}
                    onChange={(v) =>
                      store.transact(`Set let ${l.name}`, (d) => {
                        applyAddr(d, parseAddr(l.name), v);
                        const slot = d.lets?.find((x) => x.name === l.name);
                        if (slot) slot.expr = String(v);
                      })
                    }
                  />
                ) : (
                  <span class="num" title={`let ${l.name} = ${l.expr}`}>
                    {l.expr} = <strong>{fmt(l.value)}</strong>
                  </span>
                )}
              </Fragment>
            );
          })}
          <span class="hint" style={{ gridColumn: '1 / -1' }}>
            Defined as <code>let</code> in the .fsm source. Editing or sweeping a parameter recomputes every dimension and load that reads it.
          </span>
        </div>
      ) : (
        <div class="hint">No parameters defined yet. Click <code>+ let</code> to add a design variable.</div>
      )}
      <div
        class="card"
        data-testid="doc-active-equations"
        style={{
          marginTop: '10px',
          padding: '8px 10px',
          border: '1px solid var(--line-strong)',
          background: 'var(--panel-2)',
          borderRadius: '6px',
        }}
      >
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: '5px' }}>
          <span style={{ fontSize: '11px', fontWeight: 700, color: 'var(--accent)' }}>
            Activated Field Equations ({activeEqs.length})
          </span>
          <button
            type="button"
            class="btn small primary"
            data-testid="doc-open-theory"
            onClick={() => store.setLens('theory')}
            style={{ fontSize: '10px', padding: '2px 7px' }}
          >
            Theory (6) →
          </button>
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
          {activeEqs.map((eq) => (
            <div
              key={eq.id}
              style={{
                fontSize: '10.5px',
                color: 'var(--text)',
                display: 'flex',
                alignItems: 'center',
                gap: '5px',
              }}
              title={eq.triggerSummary}
            >
              <span style={{ color: 'var(--ok)', fontWeight: 700 }}>●</span>
              <span>{eq.shortName}</span>
            </div>
          ))}
        </div>
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
