// Fill lens: TPMS lattice filling of the part — pattern, mode, cell, skin, density band,
// grading curve, printable wall and protected shapes — with the graded solve's numbers next
// to it (mass in grams, achieved density window, solid-vs-graded comparison). Editing here is
// undoable like everywhere else; the analysis engine re-runs on every edit.
import { Fragment } from 'preact';
import { store } from '../doc/store';
import { drivenExpr } from '../doc/param';
import { walk } from '../doc/types';
import type { Document, Family, FillMode, Fill, HxArrangement, HxFluid, HxSpec } from '../doc/types';
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

/** Default two-stream TPMS porous-flow & conjugate HX spec using the first two regions. */
function defaultHx(d: Document): HxSpec {
  const inlet = d.regions[0]?.id ?? '';
  const outlet = d.regions.find((r) => r.id !== inlet)?.id ?? d.regions[1]?.id ?? inlet;
  return {
    inlet,
    outlet,
    mdot: 0.05,
    tin: 80,
    fluid: 'water',
    arrangement: 'counterflow',
    cold_mdot: null,
    cold_tin: 20,
    cold_fluid: null,
    pout_kpa: 100,
  };
}

/** Shape ids in the tree, depth first — `protect` names shapes (including subtracted bores/holes) so a solid margin ring is preserved around them. */
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
          {doc.root.length > 0 && s.status === 'error' && (s.errorKind === 'setup' ? <div class="hint">{s.error}</div> : <div class="error-box">{s.error}</div>)}
          {doc.root.length > 0 && s.status === 'running' && <div class="hint">analysing{s.levelLabel ? ` · ${s.levelLabel}` : '…'}</div>}

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
              {fi.lattice_adjoint_sensitivity_sum?.[0] != null && (
                <>
                  <span
                    class="plain"
                    title="Exact 3D P2 self-adjoint compliance sensitivity ∂C/∂ρ_e = −u_eᵀ (∂K_e/∂ρ_e) u_e = −(2 U_e)(∂E_e/∂ρ_e)/E_e: total stiffness gain (N·mm compliance reduction) per unit relative density increase ρ_e across lattice elements."
                  >
                    3D Adjoint Σ(−∂C/∂ρ_e)
                  </span>
                  <span class="num" data-testid="fill-adjoint-sensitivity">
                    {fi.lattice_adjoint_sensitivity_sum.map((v) => `${fmt(v, 2)} N·mm`).join(' · ')}
                  </span>
                </>
              )}
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

          <div
            data-testid="fill-hx-section"
            style={{
              marginTop: '8px',
              padding: '8px 9px',
              background: 'rgba(14, 165, 233, 0.07)',
              border: '1px solid rgba(56, 189, 248, 0.28)',
              borderRadius: '6px',
              display: 'flex',
              flexDirection: 'column',
              gap: '6px',
            }}
          >
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '6px' }}>
              <span style={{ fontSize: '11px', fontWeight: 700, color: 'var(--accent)' }}>
                💧 Porous Flow &amp; Conjugate HX
              </span>
              {doc.hx && (
                <button
                  type="button"
                  class="mini-btn"
                  data-testid="remove-hx"
                  onClick={() =>
                    store.transact('Remove porous HX', (d) => {
                      d.hx = null;
                    })
                  }
                >
                  Remove HX
                </button>
              )}
            </div>
            {!doc.hx ? (
              doc.regions.length < 2 ? (
                <div class="hint" style={{ margin: 0 }}>
                  Define at least two regions (inlet &amp; outlet) in 2. Setup to enable 3D Darcy–Forchheimer porous flow.
                </div>
              ) : (
                <button
                  type="button"
                  class="btn"
                  data-testid="add-hx"
                  onClick={() =>
                    store.transact('Enable porous HX', (d) => {
                      d.hx = defaultHx(d);
                    })
                  }
                >
                  Enable TPMS porous flow &amp; HX
                </button>
              )
            ) : (
              <>
                <div class="field-grid">
                  <label class="plain">Inlet</label>
                  <select
                    data-testid="hx-inlet"
                    value={doc.hx.inlet}
                    onChange={(e) => {
                      const next = (e.currentTarget as HTMLSelectElement).value;
                      store.transact('Set HX inlet region', (d) => {
                        if (!d.hx) return;
                        d.hx.inlet = next;
                        if (d.hx.outlet === next) {
                          const alt = d.regions.find((r) => r.id !== next)?.id;
                          if (alt) d.hx.outlet = alt;
                        }
                      });
                    }}
                  >
                    {doc.regions.map((r) => (
                      <option key={r.id} value={r.id}>
                        {r.id}
                      </option>
                    ))}
                  </select>
                  <label class="plain">Outlet</label>
                  <select
                    data-testid="hx-outlet"
                    value={doc.hx.outlet}
                    onChange={(e) => {
                      const next = (e.currentTarget as HTMLSelectElement).value;
                      store.transact('Set HX outlet region', (d) => {
                        if (!d.hx) return;
                        d.hx.outlet = next;
                        if (d.hx.inlet === next) {
                          const alt = d.regions.find((r) => r.id !== next)?.id;
                          if (alt) d.hx.inlet = alt;
                        }
                      });
                    }}
                  >
                    {doc.regions.map((r) => (
                      <option key={r.id} value={r.id}>
                        {r.id}
                      </option>
                    ))}
                  </select>
                  <label class="plain">Stream</label>
                  <Seg<HxArrangement>
                    value={doc.hx.arrangement}
                    options={[
                      { value: 'counterflow', label: 'Counter' },
                      { value: 'parallel', label: 'Parallel' },
                      { value: 'crossflow', label: 'Cross' },
                      { value: 'isothermal_wall', label: 'Wall T' },
                    ]}
                    onChange={(v) =>
                      store.transact('Set HX arrangement', (d) => {
                        if (d.hx) d.hx.arrangement = v;
                      })
                    }
                    testId="hx-arrangement"
                  />
                  <label class="plain">Fluid</label>
                  <Seg<HxFluid>
                    value={doc.hx.fluid}
                    options={[
                      { value: 'water', label: 'Water' },
                      { value: 'glycol50', label: 'Glycol' },
                      { value: 'air', label: 'Air' },
                      { value: 'he_2mpa', label: 'He 2MPa' },
                    ]}
                    onChange={(v) =>
                      store.transact('Set HX fluid', (d) => {
                        if (d.hx) d.hx.fluid = v;
                      })
                    }
                    testId="hx-fluid"
                  />
                  <NumRow
                    label="ṁ hot"
                    unit="kg/s"
                    min={1e-5}
                    step={0.01}
                    get={(d) => d.hx?.mdot ?? 0.05}
                    set={(d, v) => d.hx && (d.hx.mdot = Math.max(1e-6, v))}
                    undoLabel="Set HX mass flow"
                    testId="hx-mdot"
                    driven={drivenExpr(doc, 'hx', 'mdot')}
                    exprTarget={{ target: 'hx', attr: 'mdot' }}
                  />
                  <NumRow
                    label="T hot,in"
                    unit="°C"
                    step={5}
                    get={(d) => d.hx?.tin ?? 80}
                    set={(d, v) => d.hx && (d.hx.tin = v)}
                    undoLabel="Set HX hot inlet temperature"
                    testId="hx-tin"
                    driven={drivenExpr(doc, 'hx', 'tin')}
                    exprTarget={{ target: 'hx', attr: 'tin' }}
                  />
                  <label class="plain">Cold fluid</label>
                  <select
                    data-testid="hx-cold-fluid"
                    value={doc.hx.cold_fluid ?? ''}
                    onChange={(e) => {
                      const raw = (e.currentTarget as HTMLSelectElement).value;
                      store.transact('Set HX cold fluid', (d) => {
                        if (!d.hx) return;
                        d.hx.cold_fluid = raw ? (raw as HxFluid) : null;
                      });
                    }}
                  >
                    <option value="">Same as hot ({doc.hx.fluid})</option>
                    <option value="water">Water</option>
                    <option value="glycol50">Glycol 50/50</option>
                    <option value="air">Air</option>
                    <option value="he_2mpa">He 2 MPa</option>
                  </select>
                  <NumRow
                    label="ṁ cold"
                    unit="kg/s"
                    min={1e-5}
                    step={0.01}
                    get={(d) => d.hx?.cold_mdot ?? d.hx?.mdot ?? 0.05}
                    set={(d, v) => d.hx && (d.hx.cold_mdot = Math.max(1e-6, v))}
                    undoLabel="Set HX cold mass flow"
                    testId="hx-cold-mdot"
                    driven={drivenExpr(doc, 'hx', 'cold_mdot')}
                    exprTarget={{ target: 'hx', attr: 'cold_mdot' }}
                  />
                  <NumRow
                    label="T cold,in"
                    unit="°C"
                    step={5}
                    get={(d) => d.hx?.cold_tin ?? 20}
                    set={(d, v) => d.hx && (d.hx.cold_tin = v)}
                    undoLabel="Set HX cold inlet temperature"
                    testId="hx-cold-tin"
                    driven={drivenExpr(doc, 'hx', 'cold_tin')}
                    exprTarget={{ target: 'hx', attr: 'cold_tin' }}
                  />
                  <NumRow
                    label="p out"
                    unit="kPa"
                    min={0}
                    step={10}
                    get={(d) => d.hx?.pout_kpa ?? 101.325}
                    set={(d, v) => d.hx && (d.hx.pout_kpa = Math.max(0, v))}
                    undoLabel="Set HX outlet pressure"
                    testId="hx-pout"
                    driven={drivenExpr(doc, 'hx', 'pout')}
                    exprTarget={{ target: 'hx', attr: 'pout' }}
                  />
                </div>
                {fi?.hx && (
                  <div class="field-grid" data-testid="fill-hx-summary">
                    <span class="plain">Δp &amp; pump</span>
                    <span class="num">
                      {fmt(fi.hx.pressure_drop_kpa, 2)} kPa · {fmt(fi.hx.pumping_power_w, 2)} W (Darcy {fmt(fi.hx.darcy_share_pct, 0)}%)
                    </span>
                    <span class="plain">Thermal Q</span>
                    <span class="num">
                      {fmt(fi.hx.heat_duty_w / 1000, 2)} kW ({fmt(fi.hx.power_density_kw_per_l, 1)} kW/L) · ε {fmt(fi.hx.effectiveness * 100, 1)}%
                    </span>
                    <span class="plain">Outlets</span>
                    <span class="num">
                      hot {fmt(fi.hx.hot_outlet_temp_c, 1)} °C · cold {fmt(fi.hx.cold_outlet_temp_c, 1)} °C · UA {fmt(fi.hx.ua_w_k, 1)} W/K
                    </span>
                  </div>
                )}
              </>
            )}
          </div>

          <button
            class="btn ghost"
            data-testid="remove-fill"
            onClick={() =>
              store.transact('Remove fill', (d) => {
                d.fill = null;
                d.hx = null;
              })
            }
          >
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
    <>
      <span class="sep">|</span>
      <span class="num" data-testid="fill-badge" title={`${fmt(fi.mass_g, 2)} g (solid ${fmt(fi.solid_mass_g, 2)} g)`}>
        mass {fmt(fi.mass_g, 2)} g · deflect {fmt(defl)} mm · peak {fmt(peak)} MPa
      </span>
    </>
  );
}
statusItems.push(FillBadge);