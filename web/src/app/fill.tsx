// Fill lens: TPMS lattice filling of the part — pattern, mode, cell, skin, density band,
// grading curve, printable wall and protected shapes — with the graded solve's numbers next
// to it (mass in grams, achieved density window, solid-vs-graded comparison). Editing here is
// undoable like everywhere else; the analysis engine re-runs on every edit.
import { Fragment } from 'preact';
import { store } from '../doc/store';
import { walk } from '../doc/types';
import type { Document, Family, FillMode, Fill } from '../doc/types';
import { NumRow, Seg, VecRow, fmt } from './fields';
import { viewOverlays, statusItems } from './App';
import { analysis } from './analysis';
import { ui, useApp } from './ui';

/** Fill spec used when adding a fill (mirrors the Rust defaults for `fill`). */
function defaultFill(): Fill {
  return {
    family: 'gyroid',
    mode: 'sheet',
    cell: 6,
    skin: 1.2,
    rho_min: 0.2,
    rho_max: 0.8,
    curve: 1,
    min_wall: 0.8,
    protect: [],
    margin: 2,
    case: null,
    rotate: [0, 0, 0],
  };
}

/** Shape ids in the tree, depth first — `protect` names shapes, not regions. */
function shapeIds(d: Document): string[] {
  const ids: string[] = [];
  walk(d.root, (n) => ids.push(n.id));
  return ids;
}

/** Load-case names in document order. */
function caseNames(d: Document): string[] {
  return [...new Set(d.loads.map((l) => l.case))];
}

function FillPanel() {
  useApp();
  if (store.lens !== 'fill') return null;
  const minimized = ui.panelMinimized || ui.vpHudCollapsed;
  const toggleMinimized = () => {
    if (minimized) {
      ui.set('panelMinimized', false);
      if (ui.vpHudCollapsed) ui.set('vpHudCollapsed', false);
    } else {
      ui.set('panelMinimized', true);
    }
  };
  const doc = store.doc;
  const s = analysis.state;
  const lvl = s.levels[s.levels.length - 1];
  const fi = lvl?.meta.fill;

  return (
    <div class={`results-panel${minimized ? ' minimized' : ''}`} data-testid="fill-panel">
      <div
        class="card-head"
        style={minimized ? { marginBottom: 0, cursor: 'pointer' } : undefined}
        onClick={minimized ? toggleMinimized : undefined}
      >
        <span class="title">Fill</span>
        <span style={{ display: 'inline-flex', alignItems: 'center', gap: '6px' }} onClick={(e) => e.stopPropagation()}>
          {fi && <span class="meta">{fi.family.toLowerCase()} · {fi.mode.toLowerCase()}</span>}
          {!minimized && doc.fill && (
            <button
              type="button"
              class="mini-btn"
              data-testid="fill-inspect-interior"
              title="Toggle 35% shell translucency and section plane to inspect the interior TPMS lattice"
              onClick={() => {
                if (ui.bodyAlpha < 0.99) {
                  ui.set('bodyAlpha', 1);
                  ui.set('clip', { ...ui.clip, on: false, t: 0 });
                } else {
                  ui.set('bodyAlpha', 0.35);
                  ui.set('clip', { ...ui.clip, axis: 1, on: true, dir: 1, t: 0.5 });
                }
              }}
            >
              {ui.bodyAlpha < 0.99 ? 'Opaque shell' : 'Inspect interior'}
            </button>
          )}
          <button
            type="button"
            class="mini-btn"
            data-testid="fill-minimize-toggle"
            title={minimized ? 'Expand Fill panel' : 'Minimize Fill panel to bottom bar'}
            onClick={toggleMinimized}
          >
            {minimized ? '▴ Expand' : '▾ Min'}
          </button>
        </span>
      </div>

      {!minimized && (
        <>
          {s.status === 'error' && (s.errorKind === 'setup' ? <div class="hint">{s.error}</div> : <div class="error-box">{s.error}</div>)}
          {s.status === 'running' && <div class="hint">analysing{s.levelLabel ? ` · ${s.levelLabel}` : '…'}</div>}

          {!doc.root.length && <div class="hint">Add shapes to the model first.</div>}

          {!doc.fill && doc.root.length > 0 && (
            <>
              <div class="hint">No fill: the part is solid everywhere.</div>
              <button
                class="btn primary"
                data-testid="add-fill"
                onClick={() =>
                  store.transact('Add fill', (d) => {
                    d.fill = defaultFill();
                  })
                }
              >
                Add lattice fill
              </button>
            </>
          )}

          {doc.fill && (
        <>
          <div class="field-grid">
            <label class="plain">Pattern</label>
            <Seg<Family>
              value={doc.fill.family}
              options={[
                { value: 'gyroid', label: 'Gyroid' },
                { value: 'primitive', label: 'Primitive' },
                { value: 'diamond', label: 'Diamond' },
              ]}
              onChange={(v) =>
                store.transact('Set fill pattern', (d) => {
                  if (!d.fill) return;
                  d.fill.family = v;
                  if (d.fill.morph === v) d.fill.morph = null;
                })
              }
              testId="fill-family"
            />
            <label class="plain" title="Optional secondary TPMS family at high-stress regions (morphs smoothly from Pattern at ρmin to Morph to at ρmax)">
              Morph to
            </label>
            <Seg<'none' | Family>
              value={doc.fill.morph && doc.fill.morph !== doc.fill.family ? doc.fill.morph : 'none'}
              options={[
                { value: 'none', label: 'Same' },
                { value: 'gyroid', label: 'Gyroid' },
                { value: 'primitive', label: 'Primitive' },
                { value: 'diamond', label: 'Diamond' },
              ]}
              onChange={(v) =>
                store.transact('Set fill morph family', (d) => {
                  if (!d.fill) return;
                  d.fill.morph = v === 'none' || v === d.fill.family ? null : v;
                })
              }
              testId="fill-morph"
            />
            <label class="plain">Mode</label>
            <Seg<FillMode>
              value={doc.fill.mode}
              options={[
                { value: 'sheet', label: 'Sheet' },
                { value: 'network', label: 'Network' },
              ]}
              onChange={(v) => store.transact('Set fill mode', (d) => d.fill && (d.fill.mode = v))}
              testId="fill-mode"
            />
            <NumRow
              label="Cell"
              unit="mm"
              min={1}
              step={1}
              get={(d) => d.fill?.cell ?? 6}
              set={(d, v) => {
                if (!d.fill) return;
                d.fill.cell = v;
                if (d.fill.cell_min != null && d.fill.cell_min >= v) {
                  d.fill.cell_min = null;
                }
              }}
              undoLabel="Set fill cell"
              testId="fill-cell"
              title="Unit cell edge length at low-stress regions (ρmin)"
            />
            <NumRow
              label="Min cell"
              unit="mm"
              min={0.5}
              step={0.5}
              get={(d) => d.fill?.cell_min ?? d.fill?.cell ?? 6}
              set={(d, v) => {
                if (!d.fill) return;
                d.fill.cell_min = v >= d.fill.cell ? null : Math.max(0.5, v);
              }}
              undoLabel="Set min stress cell"
              testId="fill-cell-min"
              title="Finest unit cell edge length at high-stress regions (refines smoothly from Cell at ρmin down to Min cell at ρmax)"
            />
            <NumRow
              label="Skin"
              unit="mm"
              min={0}
              step={0.2}
              get={(d) => d.fill?.skin ?? 1.2}
              set={(d, v) => {
                if (!d.fill) return;
                d.fill.skin = v;
                if (d.fill.skin_max != null && d.fill.skin_max < v) {
                  d.fill.skin_max = null;
                }
              }}
              undoLabel="Set fill skin"
              testId="fill-skin"
              title="Solid skin kept under the outer surface"
            />
            <NumRow
              label="Max skin"
              unit="mm"
              min={0}
              step={0.2}
              get={(d) => d.fill?.skin_max ?? d.fill?.skin ?? 1.2}
              set={(d, v) => {
                if (!d.fill) return;
                d.fill.skin_max = v <= d.fill.skin ? null : v;
              }}
              undoLabel="Set max stress skin"
              testId="fill-skin-max"
              title="Peak outer skin thickness at high-stress regions (grades from Skin at ρmin to Max skin at ρmax)"
            />
            <NumRow
              label="Erode"
              unit="mm"
              min={0}
              step={0.5}
              get={(d) => d.fill?.erode ?? 0}
              set={(d, v) => {
                if (!d.fill) return;
                d.fill.erode = v <= 0 ? null : Math.max(0, v);
              }}
              undoLabel="Set level-set boundary erode"
              testId="fill-erode"
              title="Inward level-set outer boundary evolution depth in low-stress zones (0 = preserve full outer envelope; protected regions and peak load paths stay un-eroded)"
            />
            <VecRow
              label="Rotate"
              unit="°"
              step={15}
              get={(d) => d.fill?.rotate ?? [0, 0, 0]}
              set={(d, v) => d.fill && (d.fill.rotate = v)}
              undoLabel="Rotate fill lattice"
              testId="fill-rotate"
            />
            <NumRow
              label="ρ min"
              min={0.05}
              max={0.95}
              step={0.05}
              get={(d) => d.fill?.rho_min ?? 0.2}
              set={(d, v) => d.fill && (d.fill.rho_min = Math.min(v, d.fill!.rho_max))}
              undoLabel="Set minimum density"
              testId="fill-rho-min"
              title="Relative density at the lightest pocket"
            />
            <NumRow
              label="ρ max"
              min={0.05}
              max={0.95}
              step={0.05}
              get={(d) => d.fill?.rho_max ?? 0.8}
              set={(d, v) => d.fill && (d.fill.rho_max = Math.max(v, d.fill!.rho_min))}
              undoLabel="Set maximum density"
              testId="fill-rho-max"
              title="Relative density reached where the stress peaks"
            />
            <NumRow
              label="Curve"
              step={0.25}
              min={0.1}
              get={(d) => d.fill?.curve ?? 1}
              set={(d, v) => d.fill && (d.fill.curve = v)}
              undoLabel="Set grading curve"
              testId="fill-curve"
              title="Grading exponent: ρ = ρmin + (ρmax − ρmin)·(σ/σref)^curve"
            />
            <NumRow
              label="Min wall"
              unit="mm"
              step={0.1}
              min={0}
              get={(d) => d.fill?.min_wall ?? 0.8}
              set={(d, v) => d.fill && (d.fill.min_wall = v)}
              undoLabel="Set minimum printable wall"
              testId="fill-wall"
              title="Thinnest printable wall; light walls are thickened to reach it"
            />
            <NumRow
              label="Protect margin"
              unit="mm"
              step={0.5}
              min={0}
              get={(d) => d.fill?.margin ?? 2}
              set={(d, v) => d.fill && (d.fill.margin = v)}
              undoLabel="Set protect margin"
              testId="fill-margin"
              title="Material kept solid within this of a protected shape"
            />
            <label class="plain">Grade by</label>
            <Seg<'env' | string>
              value={doc.fill.case === null ? 'env' : doc.fill.case}
              options={[
                { value: 'env', label: 'Envelope' },
                ...caseNames(doc).map((c) => ({ value: c, label: c })),
              ]}
              onChange={(v) => store.transact('Set grading case', (d) => d.fill && (d.fill.case = v === 'env' ? null : v))}
              testId="fill-case"
            />
            {shapeIds(doc).length > 0 && (
              <>
                <label class="plain">Protect</label>
                <div class="chips" data-testid="fill-protect">
                  {shapeIds(doc).map((id) => {
                    const on = doc.fill!.protect.includes(id);
                    return (
                      <button
                        key={id}
                        data-value={id}
                        class={on ? 'on' : ''}
                        title={on ? `Keep ${id} solid inside its margin — click to release` : `Keep shape ${id} solid — click to protect`}
                        onClick={() =>
                          store.transact(on ? 'Release protected shape' : 'Protect shape', (d) => {
                            if (!d.fill) return;
                            d.fill.protect = on ? d.fill.protect.filter((p) => p !== id) : [...d.fill.protect, id];
                          })
                        }
                      >
                        {id}
                      </button>
                    );
                  })}
                </div>
              </>
            )}
          </div>

          {fi && (
            <div class="field-grid" data-testid="fill-summary">
              <span class="plain">Mass</span>
              <span class="num">
                {fmt(fi.mass_g, 2)} g · solid {fmt(fi.solid_mass_g, 2)} g (−{fmt(100 * (1 - fi.mass_g / fi.solid_mass_g), 0)}%)
              </span>
              <span class="plain">Achieved design ρ</span>
              <span class="num">{fmt(fi.rho_design[0], 3)}…{fmt(fi.rho_design[1], 3)}</span>
              <span class="plain" title="Grading normalisation: 90th percentile of the driving stress over lattice-region elements. Protection changes it, since it shrinks that region.">σ ref</span>
              <span class="num">{fmt(fi.sigma_ref, 2)} MPa</span>
              {(lvl?.meta.solid_cases ?? []).map((sc, i) => {
                const gc = lvl?.meta.cases[i];
                if (!lvl || !gc || i >= 4) return null;
                const sd = 100 * (gc.max_displacement - sc.max_displacement) / Math.max(1e-9, sc.max_displacement);
                const sv = 100 * (gc.max_vm_elem - sc.max_vm_elem) / Math.max(1e-9, sc.max_vm_elem);
                return (
                  <Fragment key={`f${sc.name}`}>
                    <span class="plain">{sc.name} vs solid</span>
                    <span class="num">
                      deflect {sd >= 0 ? '+' : ''}{fmt(sd, 1)}% · peak {sv >= 0 ? '+' : ''}{fmt(sv, 1)}%
                    </span>
                  </Fragment>
                );
              })}
            </div>
          )}

          <button class="btn ghost" data-testid="remove-fill" onClick={() => store.transact('Remove fill', (d) => (d.fill = null))}>
            Remove fill
          </button>
            </>
          )}
        </>
      )}
    </div>
  );
}
viewOverlays.push(FillPanel);

// Status-bar line for the fill lens: the graded solve's headline numbers, same shape as the
// results badge but from the fill level in the fill lens.
function FillBadge() {
  useApp();
  if (store.lens !== 'fill') return null;
  const s = analysis.state;
  if (s.status !== 'ready') return null;
  const lvl = s.levels[s.levels.length - 1];
  const fi = lvl?.meta.fill;
  if (!lvl || !fi) return null;
  const peak = Math.max(1e-9, ...lvl.meta.cases.map((c) => c.max_vm_elem));
  const defl = Math.max(1e-9, ...lvl.meta.cases.map((c) => c.max_displacement));
  return (
    <span class="num" data-testid="fill-badge" title={`${fmt(fi.mass_g, 2)} g (solid ${fmt(fi.solid_mass_g, 2)} g)`}>
      mass {fmt(fi.mass_g, 2)} g · deflect {fmt(defl)} mm · peak {fmt(peak)} MPa
    </span>
  );
}
statusItems.push(FillBadge);