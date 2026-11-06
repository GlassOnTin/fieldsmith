// Interactive 3-Column Tutorial Guide & Theoretical Manual (T1–T8):
// 1) Step-by-step Human User Tutorial (with 1-click Lens navigation & model loader)
// 2) Textbook Theoretical Manual Chapter (governing equations, discretisation & V01–V52 proofs)
// 3) Live MCP / REST API Flow (with 1-click execution against `POST /api/mcp` & shared workspace sync)

import { useState } from 'preact/hooks';
import { applyAddrs, parseAddr } from '../doc/param';
import { store, type Lens } from '../doc/store';
import {
  collectVolumeContinuumNodes,
  evaluateDocumentFieldEquations,
  getContinuumMaterialSpec,
} from '../doc/continuum';
import { parseFsm, printFsm } from '../doc/wasm';
import { EXAMPLES, extraCommands, newDocument, openFile, openText, viewApi } from './commands';
import { viewOverlays, statusItems } from './App';
import { markWorkspaceRevisionSeen, swirlShared } from './systems';
import { apiUrl, ui, useApp } from './ui';
import { FixingTorqueAndWeibullWorkbench } from './FixingsModal';
import { SketchWorkbench } from './SketchWorkbench';
import {
  openAntennaWorkbenchModal,
  syncDeadCatDroneToDocument,
} from './AntennaWorkbench';
import {
  analyzeDroneAntenna,
  defaultOptimisedDroneAntennaSpec,
  extractDroneAntennaSpecFromDoc,
  optimiseDroneAntenna,
  unoptimisedTuckedDroneAntennaSpec,
} from '../doc/antenna-vna';

import {
  TUTORIAL_SHEETS,
  formatLiterateFsm,
  parseLiterateFsm,
  getCategoryForSheet,
  getDocPathForSheet,
  getFsmPathForSheet,
  getVerificationRefsForSheet,
  getCategoryLabel,
  type TutorialStep,
  type TheorySection,
  type McpFlowStep,
  type TutorialCategory,
  type AnalyticCheckRow,
  type TutorialSheet,
} from "../doc/literate-fsm";

export {
  TUTORIAL_SHEETS,
  formatLiterateFsm,
  parseLiterateFsm,
  getCategoryForSheet,
  getDocPathForSheet,
  getFsmPathForSheet,
  getVerificationRefsForSheet,
  getCategoryLabel,
  type TutorialStep,
  type TheorySection,
  type McpFlowStep,
  type TutorialCategory,
  type AnalyticCheckRow,
  type TutorialSheet,
};

export function renderSheetMarkdown(sheet: TutorialSheet): string {
  const cat = getCategoryForSheet(sheet);
  const docPath = getDocPathForSheet(sheet);
  const fsmPath = getFsmPathForSheet(sheet);
  const vRefs = getVerificationRefsForSheet(sheet);
  const lines: string[] = [];

  lines.push(`# ${sheet.code} — ${sheet.title}`);
  lines.push('');
  lines.push(`> **Category**: ${getCategoryLabel(cat)} · **Level**: ${sheet.level} · **Verification**: ${sheet.verificationBadge}`);
  lines.push(`> **Literate .fsm Source**: \`${fsmPath}\` · **Repository Markdown**: \`${docPath}\`${vRefs.length > 0 ? ` · **Verification Reports**: ${vRefs.map((r) => `\`${r}\``).join(', ')}` : ''}`);
  lines.push('');
  lines.push(sheet.subtitle);
  lines.push('');

  if (sheet.analyticChecks && sheet.analyticChecks.length > 0) {
    lines.push('## Closed-Form Analytic Reference vs Fieldsmith P2 FEA');
    lines.push('');
    lines.push('| Quantity | Closed-Form Formula | Analytic Reference | Fieldsmith Measured | Difference | Tolerance |');
    lines.push('| :--- | :--- | ---: | ---: | ---: | :--- |');
    for (const row of sheet.analyticChecks) {
      lines.push(
        `| ${row.quantity} | \`${row.formula}\` | **${row.analyticRef}** | **${row.measuredFea}** | ${row.relError} | ${row.tolerance} |`,
      );
    }
    lines.push('');
  }

  lines.push('## 1. Starter `.fsm` Model');
  lines.push('');
  lines.push('Save or load this script in the Fieldsmith UI (**Tutorials & Benchmarks** drawer) or run via CLI (`fieldsmith solve --report`):');
  lines.push('');
  lines.push('```fsm');
  lines.push(sheet.starterFsm.trim());
  lines.push('```');
  lines.push('');

  lines.push('## 2. Step-by-Step Guided Walkthrough');
  lines.push('');
  sheet.userSteps.forEach((st) => {
    const lensBadge = st.lens ? ` *(Lens: \`${st.lens.toUpperCase()}\`)*` : '';
    lines.push(`### ${st.title}${lensBadge}`);
    lines.push('');
    lines.push(st.instruction);
    lines.push('');
    lines.push(`- **Expected Check**: ${st.expected}`);
    lines.push('');
  });

  lines.push(`## 3. Theoretical Manual — ${sheet.theoryChapter.chapterTitle}`);
  lines.push('');
  lines.push(sheet.theoryChapter.summary);
  lines.push('');
  for (const sec of sheet.theoryChapter.sections) {
    lines.push(`### ${sec.heading}`);
    lines.push('');
    lines.push('```text');
    lines.push(sec.equations.trim());
    lines.push('```');
    lines.push('');
    lines.push(sec.body);
    lines.push('');
  }

  lines.push('## 4. Reproducible Automation (MCP / REST API & CLI)');
  lines.push('');
  for (const ms of sheet.mcpSteps) {
    lines.push(`### ${ms.title} (\`${ms.tool}\`)`);
    lines.push('');
    lines.push(ms.description);
    lines.push('');
    lines.push('```json');
    lines.push(JSON.stringify({ tool: ms.tool, arguments: ms.arguments }, null, 2));
    lines.push('```');
    lines.push('');
  }

  return lines.join('\n');
}

export function loadTutorialSheetModel(sheetId: string, openSystemsIfFlagged = false) {
  const sheet = TUTORIAL_SHEETS.find((s) => s.id === sheetId) ?? TUTORIAL_SHEETS[0];
  ui.set('bodyAlpha', 1);
  if (sheet.id === 'T9' || sheet.id === 'T10') {
    ui.set('clip', { axis: 0, dir: 1, on: true, t: 0 });
  } else {
    ui.set('clip', { axis: 1, dir: 1, on: false, t: 0 });
  }
  const ok = openText(sheet.starterFsm, sheet.starterFsmName);
  viewApi.fit(false);
  if (openSystemsIfFlagged && sheet.opensSystemsWorkbench) {
    ui.set('systemsOpen', true);
  } else {
    ui.set('systemsOpen', false);
    store.setLens('shape');
  }
  if (ok) {
    ui.notify(`Loaded ${sheet.code}: ${sheet.title}`);
    void fetch(apiUrl('/api/workspace'), {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        actor: 'web-ui',
        last_tool: `load-${sheet.code}`,
        summary: `Loaded ${sheet.code}: ${sheet.title}`,
        doc_fsm: printFsm(store.doc),
      }),
    })
      .then((r) => r.json())
      .then((ws) => {
        if (typeof ws?.revision === 'number') markWorkspaceRevisionSeen(ws.revision);
      })
      .catch(() => {});
  }
}

export function toggleTutorialsDrawer() {
  if (ui.tutorialsOpen && ui.tutorialMinimised) {
    ui.set('tutorialMinimised', false);
    return;
  }
  const nextOpen = !ui.tutorialsOpen;
  ui.set('tutorialMinimised', false);
  ui.set('tutorialsOpen', nextOpen);
  if (nextOpen) {
    const activeSheet = TUTORIAL_SHEETS.find((s) => s.id === ui.tutorialSheetId) ?? TUTORIAL_SHEETS[0];
    // If the viewport is empty, holds the Float-Bath from a previous session, or doesn't match the active tutorial, load the active tutorial sheet immediately.
    const curName = store.doc.name || '';
    if (
      store.doc.root.length === 0 ||
      (curName.includes('Float-Bath') && activeSheet.id !== 'T8') ||
      !curName.startsWith(activeSheet.code)
    ) {
      loadTutorialSheetModel(activeSheet.id, false);
    }
  }
}

function computeSwirlCombustorNox(params: {
  orificeR: number;
  swirlSlotR: number;
  swirlOffset: number;
  linerWall: number;
  coolantTemp: number;
  fuelPressureBar: number;
  equivRatio: number;
}) {
  const { orificeR, swirlSlotR, swirlOffset, linerWall, coolantTemp, fuelPressureBar, equivRatio } = params;
  // Geometric Swirl Number S_g = (R_offset * r_orifice) / (n_ports * r_slot^2) with n_ports = 2
  const swirlNumber = (swirlOffset * orificeR) / Math.max(0.5, 2.0 * swirlSlotR * swirlSlotR);
  // Hollow-cone spray half-angle (deg)
  const sprayHalfAngleDeg = Math.min(72.0, Math.max(22.0, 18.0 + 23.5 * Math.pow(swirlNumber, 0.56)));
  // Air-core liquid film thickness at the orifice lip (um)
  const filmThicknessUm =
    295.0 *
    Math.pow(orificeR / 2.6, 0.62) *
    Math.pow(12.0 / Math.max(2.0, fuelPressureBar), 0.28) *
    Math.pow(1.65 / Math.max(0.35, swirlNumber), 0.42);
  // Lefebvre Sauter Mean Diameter D_32 (um)
  const smdUm =
    46.5 *
    Math.pow(filmThicknessUm / 255.0, 0.38) *
    Math.pow(12.0 / Math.max(2.0, fuelPressureBar), 0.32) *
    Math.pow(orificeR / 2.6, 0.28) *
    Math.pow(Math.cos((sprayHalfAngleDeg * Math.PI) / 360.0), 0.25);
  // D^2-law droplet evaporation time tau_evap = D_32^2 / K_evap (ms, with K_evap = 0.90 mm^2/s)
  const evapTimeMs = (smdUm * smdUm * 1e-3) / 0.9;
  // Mixture-fraction unmixedness s_phi driven by slow droplet evaporation
  const unmixedness = Math.min(0.38, 0.02 + 0.085 * Math.pow(evapTimeMs, 0.95));
  // Homogeneous lean adiabatic flame temp vs stoichiometric diffusion-pocket temp (2350 K)
  const tAdiabaticK = 1280.0 + 1020.0 * equivRatio - 260.0 * Math.pow(Math.max(0, equivRatio - 1.02), 2);
  const diffWeight = Math.min(0.75, 1.95 * unmixedness);
  const hxCoolingDropK = 22.0 * Math.pow(5.0 / Math.max(2.0, linerWall), 0.35) * ((180.0 - coolantTemp) / 120.0);
  const tFlameEffK = (1.0 - diffWeight) * tAdiabaticK + diffWeight * 2350.0 - hxCoolingDropK;
  // Conjugate hot-side combustor liner wall temperature (deg C)
  const linerHotWallC = Math.round(
    coolantTemp + (tFlameEffK - 273.15 - coolantTemp) / (1.0 + 1.85 * Math.pow(5.0 / Math.max(2.0, linerWall), 0.42)),
  );
  // Extended Zeldovich Thermal NOx + Fenimore Prompt NOx (ppm @ 15% O2)
  const promptNoxPpm = 3.6 * Math.pow(equivRatio, 1.3);
  const o2RadicalFactor = Math.sqrt(Math.max(0.015, 1.08 - equivRatio));
  const residenceMs = 3.2;
  const thermalNoxPpm =
    6.8e11 * (residenceMs * 1e-3) * o2RadicalFactor * Math.exp(-38200.0 / Math.max(1200.0, tFlameEffK));
  const totalNoxPpm = promptNoxPpm + thermalNoxPpm;
  // Heat-exchanger thermal recovery power (kW)
  const hxPowerKw = 14.5 + 0.052 * (linerHotWallC - coolantTemp) * Math.pow(5.0 / Math.max(2.0, linerWall), 0.3);

  return {
    swirlNumber,
    sprayHalfAngleDeg,
    filmThicknessUm,
    smdUm,
    evapTimeMs,
    tFlameEffK,
    linerHotWallC,
    totalNoxPpm,
    hxPowerKw,
  };
}

function SwirlAtomiserNoxPredictor() {
  useApp();
  const [fuelPressureBar, setFuelPressureBar] = useState<number>(12.0);
  const [equivRatio, setEquivRatio] = useState<number>(0.88);

  const getLet = (name: string, fallback: number): number => {
    const found = (store.doc.lets ?? []).find((l) => l.name === name);
    return found && Number.isFinite(found.value) ? found.value : fallback;
  };

  const orificeR = getLet('orifice_r', 3.2);
  const swirlSlotR = getLet('swirl_slot_r', 2.4);
  const swirlOffset = getLet('swirl_offset', 5.5);
  const linerWall = getLet('liner_wall', 5.0);
  const coolantTemp = getLet('coolant_temp', 60.0);

  const metrics = computeSwirlCombustorNox({
    orificeR,
    swirlSlotR,
    swirlOffset,
    linerWall,
    coolantTemp,
    fuelPressureBar,
    equivRatio,
  });

  const applyNozzleLet = (updates: Record<string, number>, nextPBar = fuelPressureBar, nextPhi = equivRatio) => {
    swirlShared.overlay.fuelPressureBar = nextPBar;
    swirlShared.overlay.equivRatio = nextPhi;
    const nextOrifice = updates.orifice_r ?? orificeR;
    const nextSlot = updates.swirl_slot_r ?? swirlSlotR;
    const nextOffset = updates.swirl_offset ?? swirlOffset;
    const nextWall = updates.liner_wall ?? linerWall;
    const nextCool = updates.coolant_temp ?? coolantTemp;
    const nextMetrics = computeSwirlCombustorNox({
      orificeR: nextOrifice,
      swirlSlotR: nextSlot,
      swirlOffset: nextOffset,
      linerWall: nextWall,
      coolantTemp: nextCool,
      fuelPressureBar: nextPBar,
      equivRatio: nextPhi,
    });
    const allUpdates: Record<string, number> = {
      ...updates,
      flame_temp: nextMetrics.linerHotWallC,
    };
    store.transact('Tune swirl atomiser & NOx parameters', (d) => {
      const pairs: [ReturnType<typeof parseAddr>, number][] = [];
      for (const [k, v] of Object.entries(allUpdates)) {
        if ((d.lets ?? []).some((l) => l.name === k)) {
          pairs.push([parseAddr(k), v]);
        }
      }
      if (pairs.length > 0) {
        applyAddrs(d, pairs, true);
      }
    });
  };

  const noxPass = metrics.totalNoxPpm <= 25.0;

  return (
    <div class="tut-step-card" data-testid="t9-nox-predictor" style={{ borderColor: 'var(--accent)' }}>
      <div class="tut-step-top">
        <strong>Interactive Swirl Atomiser Spray &amp; NOx Predictor (Synced to 3D CAD)</strong>
        <span
          class={`num ${noxPass ? 'ok-pill' : 'bad-pill'}`}
          data-testid="t9-nox-badge"
          style={{
            padding: '2px 7px',
            borderRadius: '4px',
            fontWeight: 700,
            fontSize: '11px',
            background: noxPass ? 'color-mix(in srgb, var(--ok) 18%, transparent)' : 'color-mix(in srgb, var(--danger) 18%, transparent)',
            color: noxPass ? 'var(--ok)' : 'var(--danger)',
          }}
        >
          NOx: {metrics.totalNoxPpm.toFixed(1)} ppm {noxPass ? '(PASS ≤ 25 ppm)' : '(HIGH > 25 ppm)'}
        </span>
      </div>

      <div style={{ display: 'flex', gap: '6px', flexWrap: 'wrap', margin: '2px 0' }}>
        <button
          type="button"
          class="btn small"
          data-testid="t9-preset-coarse"
          onClick={() => {
            setFuelPressureBar(12.0);
            setEquivRatio(0.88);
            applyNozzleLet({ orifice_r: 3.2, swirl_slot_r: 2.4, swirl_offset: 5.5, liner_wall: 5.0, coolant_temp: 60 }, 12.0, 0.88);
          }}
        >
          Baseline Coarse Spray (44.3 ppm)
        </button>
        <button
          type="button"
          class="btn small primary"
          data-testid="t9-preset-low-nox"
          onClick={() => {
            setFuelPressureBar(22.0);
            setEquivRatio(0.78);
            applyNozzleLet({ orifice_r: 2.1, swirl_slot_r: 1.6, swirl_offset: 7.2, liner_wall: 4.2, coolant_temp: 55 }, 22.0, 0.78);
          }}
        >
          Apply Low-NOx Fine-Swirl Nozzle (18.5 ppm)
        </button>
      </div>

      {/* Live Multi-Physics Readout Strip */}
      <div
        class="num"
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(115px, 1fr))',
          gap: '6px',
          padding: '6px 8px',
          borderRadius: '5px',
          background: 'var(--panel)',
          border: '1px solid var(--line)',
          fontSize: '11px',
        }}
      >
        <div>
          <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Swirl Number S_g</div>
          <strong>{metrics.swirlNumber.toFixed(2)}</strong> ({metrics.sprayHalfAngleDeg.toFixed(0)}° cone)
        </div>
        <div>
          <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Droplet SMD (D₃₂)</div>
          <strong data-testid="t9-smd-readout">{metrics.smdUm.toFixed(1)} μm</strong>
        </div>
        <div>
          <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Evap Time (τ_evap)</div>
          <strong>{metrics.evapTimeMs.toFixed(2)} ms</strong>
        </div>
        <div>
          <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Flame Zone T_eff</div>
          <strong>{metrics.tFlameEffK.toFixed(0)} K</strong> ({(metrics.tFlameEffK - 273.15).toFixed(0)} °C)
        </div>
        <div>
          <div style={{ color: 'var(--text-3)', fontSize: '10px' }}>Liner Wall / HX</div>
          <strong>{metrics.linerHotWallC} °C</strong> ({metrics.hxPowerKw.toFixed(1)} kW)
        </div>
      </div>

      {/* Interactive Nozzle & Combustor Sliders */}
      <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))', gap: '6px 12px', fontSize: '11px' }}>
        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>Orifice Radius (orifice_r)</span>
            <strong class="num">{orificeR.toFixed(1)} mm</strong>
          </div>
          <input
            type="range"
            min="1.8"
            max="4.4"
            step="0.1"
            value={orificeR}
            data-testid="t9-slider-orifice"
            onInput={(e) => applyNozzleLet({ orifice_r: parseFloat((e.target as HTMLInputElement).value) })}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>Swirl Port Radius (swirl_slot_r)</span>
            <strong class="num">{swirlSlotR.toFixed(1)} mm</strong>
          </div>
          <input
            type="range"
            min="1.3"
            max="3.0"
            step="0.1"
            value={swirlSlotR}
            data-testid="t9-slider-slot"
            onInput={(e) => applyNozzleLet({ swirl_slot_r: parseFloat((e.target as HTMLInputElement).value) })}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>Swirl Lever Arm (swirl_offset)</span>
            <strong class="num">{swirlOffset.toFixed(1)} mm</strong>
          </div>
          <input
            type="range"
            min="3.5"
            max="8.0"
            step="0.1"
            value={swirlOffset}
            data-testid="t9-slider-offset"
            onInput={(e) => applyNozzleLet({ swirl_offset: parseFloat((e.target as HTMLInputElement).value) })}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>Fuel Pressure (ΔP_fuel)</span>
            <strong class="num">{fuelPressureBar.toFixed(0)} bar</strong>
          </div>
          <input
            type="range"
            min="6"
            max="32"
            step="1"
            value={fuelPressureBar}
            data-testid="t9-slider-pressure"
            onInput={(e) => {
              const p = parseFloat((e.target as HTMLInputElement).value);
              setFuelPressureBar(p);
              applyNozzleLet({}, p, equivRatio);
            }}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>Primary Equivalence Ratio (φ_pz)</span>
            <strong class="num">{equivRatio.toFixed(2)}</strong>
          </div>
          <input
            type="range"
            min="0.65"
            max="1.05"
            step="0.01"
            value={equivRatio}
            data-testid="t9-slider-phi"
            onInput={(e) => {
              const phi = parseFloat((e.target as HTMLInputElement).value);
              setEquivRatio(phi);
              applyNozzleLet({}, fuelPressureBar, phi);
            }}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
          <div style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>Liner Wall Thickness (liner_wall)</span>
            <strong class="num">{linerWall.toFixed(1)} mm</strong>
          </div>
          <input
            type="range"
            min="3.0"
            max="7.0"
            step="0.2"
            value={linerWall}
            data-testid="t9-slider-wall"
            onInput={(e) => applyNozzleLet({ liner_wall: parseFloat((e.target as HTMLInputElement).value) })}
          />
        </label>
      </div>
    </div>
  );
}

type ColumnTab = 'user' | 'theory' | 'mcp' | 'markdown' | 'submit';

export type TutorialTrackId =
  | 'analytic_benchmarks'
  | 'foundations'
  | 'structural_thermal'
  | 'lattice_topology'
  | 'multiphysics'
  | 'examples';

export interface TutorialTrackMeta {
  id: TutorialTrackId;
  number: string;
  title: string;
  subtitle: string;
  accent: string;
}

export const TUTORIAL_TRACKS: TutorialTrackMeta[] = [
  {
    id: 'analytic_benchmarks',
    number: 'Track 0',
    title: 'Closed-Form Analytic Verification Benchmarks (A1–A14 · V1–V60)',
    subtitle: 'Basilisk/Gerris-style literate verification across all physics: beams, Kirsch, Pappus, Lamé, Robin–Neumann thermo-elasticity, modal doublets, distributed fn loads, dual compliance bounds, series composites, TPMS cubic tensors, additive peel, fasteners, and 50 Ω RF.',
    accent: '#06b6d4',
  },
  {
    id: 'foundations',
    number: 'Track I',
    title: 'Foundations, CSG Geometry & Parametric Sweeps',
    subtitle: 'Signed distance primitives, boolean fillets, region supports/loads, P2 stress, and let-variable trade-off sweeps.',
    accent: '#3b82f6',
  },
  {
    id: 'structural_thermal',
    number: 'Track II',
    title: 'Thermal Conduction, Modal Vibration, Additive Peel & Bolted Assemblies',
    subtitle: 'Steady-state Fourier conduction, Duhamel–Neumann thermal strain, Lanczos natural frequencies, Tsai–Wu layer peel, and multi-material bolt preload.',
    accent: '#f59e0b',
  },
  {
    id: 'lattice_topology',
    number: 'Track III',
    title: 'Implicit TPMS Minimal Surfaces, Multi-Zone Morphing & Level-Set Topology',
    subtitle: 'Two-pass homogenised Gyroid/Primitive/Diamond lattices, spatially varying unit-cell wavelength, and outer boundary level-set evolution.',
    accent: '#10b981',
  },
  {
    id: 'multiphysics',
    number: 'Track IV',
    title: 'Coupled Multi-Physics & Open-Hardware Case Studies',
    subtitle: 'Regeneratively cooled rocket chambers, liquid-metal MHD, VFT glass, swirl combustion, solar optics, and CFRP drone RF antennas.',
    accent: '#f97316',
  },
  {
    id: 'examples',
    number: 'Track V',
    title: 'Standalone Reference .fsm Starter Models',
    subtitle: 'Compact, self-contained .fsm scripts ready to open, modify, or use as templates for new simulations.',
    accent: '#a855f7',
  },
];

function getTrackForSheet(sheetId: string): TutorialTrackId {
  if (sheetId.startsWith('A')) return 'analytic_benchmarks';
  switch (sheetId) {
    case 'T1':
    case 'T2':
    case 'T15':
      return 'foundations';
    case 'T3':
    case 'T4':
    case 'T5':
    case 'T10':
    case 'T14':
      return 'structural_thermal';
    case 'T6':
    case 'T7':
    case 'T11':
    case 'T12':
      return 'lattice_topology';
    case 'T8':
    case 'T9':
    case 'T13':
    case 'T16':
    case 'T17':
    default:
      return 'multiphysics';
  }
}

interface IntrospectedFsmSummary {
  shapeCount: number;
  regionCount: number;
  bcCount: number;
  letCount: number;
  materials: { id: string; name: string; phaseLabel: string; color: string }[];
  activeEquations: { id: string; shortName: string }[];
  fillSummary: string | null;
}

const fsmIntrospectionCache = new Map<string, IntrospectedFsmSummary>();

/** Automatically parse any .fsm script and extract its continuum materials, phases,
 * active governing field equations, and geometry/BC counts so the Contents page updates
 * automatically whenever new tutorials or examples are added. */
function inspectFsmCatalogEntry(key: string, fsmText: string): IntrospectedFsmSummary {
  const cached = fsmIntrospectionCache.get(key);
  if (cached) return cached;

  try {
    const parsed = parseFsm(fsmText);
    if ('ok' in parsed) {
      const doc = parsed.ok;
      const vols = collectVolumeContinuumNodes(doc);
      const eqs = evaluateDocumentFieldEquations(doc).filter((e) => e.active);
      const matMap = new Map<string, { id: string; name: string; phaseLabel: string; color: string }>();
      const baseSpec = getContinuumMaterialSpec(doc, doc.material);
      matMap.set(doc.material, {
        id: doc.material,
        name: baseSpec.name,
        phaseLabel: baseSpec.phaseLabel,
        color: baseSpec.badgeColor,
      });
      for (const v of vols) {
        if (!matMap.has(v.matId)) {
          matMap.set(v.matId, {
            id: v.matId,
            name: v.spec.name,
            phaseLabel: v.spec.phaseLabel,
            color: v.spec.badgeColor,
          });
        }
      }
      const fillSummary = doc.fill
        ? `${doc.fill.family}${doc.fill.morph ? ` → ${doc.fill.morph}` : ''} (${doc.fill.cell}${doc.fill.cell_min ? `→${doc.fill.cell_min}` : ''} mm${doc.fill.erode ? `, erode ${doc.fill.erode} mm` : ''})`
        : null;
      const summary: IntrospectedFsmSummary = {
        shapeCount: vols.length,
        regionCount: doc.regions.length,
        bcCount:
          doc.supports.length +
          doc.loads.length +
          doc.temperatures.length +
          doc.convections.length +
          doc.heat_fluxes.length,
        letCount: doc.lets?.length ?? 0,
        materials: Array.from(matMap.values()),
        activeEquations: eqs.map((e) => ({ id: e.id, shortName: e.shortName })),
        fillSummary,
      };
      fsmIntrospectionCache.set(key, summary);
      return summary;
    }
  } catch {
    // ignore before wasm ready
  }
  return {
    shapeCount: 0,
    regionCount: 0,
    bcCount: 0,
    letCount: 0,
    materials: [],
    activeEquations: [],
    fillSummary: null,
  };
}

function TutorialsContentsPage({
  onSelectSheet,
  onOpenSheetMarkdown,
  onLoadSheetModelOnly,
  onOpenSheetTheory,
  onOpenSubmitBuilder,
}: {
  onSelectSheet: (sheetId: string) => void;
  onOpenSheetMarkdown: (sheetId: string) => void;
  onLoadSheetModelOnly: (sheetId: string) => void;
  onOpenSheetTheory: (sheetId: string) => void;
  onOpenSubmitBuilder: () => void;
}) {
  const [searchQuery, setSearchQuery] = useState('');
  const categoryFilter = ui.tutorialCategoryFilter;
  const setCategoryFilter = (cat: 'all' | TutorialCategory | 'example') =>
    ui.set('tutorialCategoryFilter', cat);
  const [trackFilter, setTrackFilter] = useState<'all' | TutorialTrackId>('all');

  const q = searchQuery.trim().toLowerCase();

  const matchesSheet = (s: TutorialSheet, info: IntrospectedFsmSummary): boolean => {
    const cat = getCategoryForSheet(s);
    if (categoryFilter === 'example') return false;
    if (categoryFilter !== 'all' && categoryFilter !== cat) return false;
    const track = getTrackForSheet(s.id);
    if (trackFilter !== 'all' && trackFilter !== track) return false;
    if (!q) return true;
    const haystack = [
      s.code,
      s.title,
      s.subtitle,
      s.level,
      s.verificationBadge,
      s.starterFsmName,
      getDocPathForSheet(s),
      ...getVerificationRefsForSheet(s),
      s.theoryChapter.chapterTitle,
      ...info.materials.map((m) => `${m.id} ${m.name} ${m.phaseLabel}`),
      ...info.activeEquations.map((e) => e.shortName),
      info.fillSummary ?? '',
    ]
      .join(' ')
      .toLowerCase();
    return haystack.includes(q);
  };

  const matchesExample = (ex: (typeof EXAMPLES)[number], info: IntrospectedFsmSummary): boolean => {
    if (categoryFilter !== 'all' && categoryFilter !== 'example') return false;
    if (trackFilter !== 'all' && trackFilter !== 'examples') return false;
    if (!q) return true;
    const haystack = [
      ex.id,
      ex.label,
      ...info.materials.map((m) => `${m.id} ${m.name}`),
      ...info.activeEquations.map((e) => e.shortName),
    ]
      .join(' ')
      .toLowerCase();
    return haystack.includes(q);
  };

  const totalCount = TUTORIAL_SHEETS.length + EXAMPLES.length;
  const tutorialCount = TUTORIAL_SHEETS.filter((s) => getCategoryForSheet(s) === 'tutorial').length;
  const analyticCount = TUTORIAL_SHEETS.filter((s) => getCategoryForSheet(s) === 'analytic').length;
  const caseStudyCount = TUTORIAL_SHEETS.filter((s) => getCategoryForSheet(s) === 'case-study').length;

  return (
    <div
      data-testid="tutorials-contents-page"
      style={{
        padding: '14px 16px 28px',
        overflowY: 'auto',
        display: 'flex',
        flexDirection: 'column',
        gap: '16px',
        flex: 1,
      }}
    >
      {/* Top Directory Banner + Search, Category & Track Filter Bar */}
      <div
        data-testid="tutorial-contents-filter-bar"
        style={{
          display: 'flex',
          flexDirection: 'column',
          gap: '10px',
          padding: '12px 14px',
          borderRadius: '8px',
          background: 'var(--panel-2)',
          border: '1px solid var(--line-strong)',
        }}
      >
        <div style={{ display: 'flex', flexWrap: 'wrap', justifyContent: 'space-between', alignItems: 'center', gap: '10px' }}>
          <div>
            <div style={{ fontSize: '15px', fontWeight: 700, color: 'var(--text)' }}>
              Guided Tutorials, Analytic Verification Benchmarks &amp; Multiphysics Case Studies ({totalCount} Total)
            </div>
            <div style={{ fontSize: '11.5px', color: 'var(--text-2)', marginTop: '2px' }}>
              Categorised into <strong>Guided Tutorials</strong> (<code>docs/tutorials/</code>), <strong>Analytic Verification Tests</strong> (<code>docs/analytic-tests/</code> + <code>docs/verification/V*.md</code>), <strong>Multiphysics Case Studies</strong> (<code>docs/case-studies/</code>), and <strong>Starter Examples</strong> (<code>examples/</code>).
            </div>
          </div>
          <div style={{ display: 'flex', gap: '6px', alignItems: 'center' }}>
            <input
              type="search"
              class="text-input"
              placeholder="Search tutorials, analytic tests (Lame, Kirsch, V60), or materials…"
              value={searchQuery}
              data-testid="tutorial-contents-search"
              onInput={(e) => setSearchQuery((e.target as HTMLInputElement).value)}
              style={{ width: 'min(340px, 75vw)', fontSize: '12px', padding: '5px 10px', background: 'var(--panel)' }}
            />
            {searchQuery && (
              <button
                type="button"
                class="btn ghost small"
                onClick={() => setSearchQuery('')}
              >
                Clear
              </button>
            )}
          </div>
        </div>

        {/* Top-Level Category Filter Pills */}
        <div
          data-testid="tutorial-category-filter"
          style={{ display: 'flex', flexWrap: 'wrap', gap: '6px', alignItems: 'center' }}
        >
          <span style={{ fontSize: '10.5px', fontWeight: 700, color: 'var(--text-3)', fontFamily: 'var(--mono)' }}>
            CATEGORY:
          </span>
          {(
            [
              { id: 'all', label: `All (${totalCount})` },
              { id: 'tutorial', label: `📘 Guided Tutorials (${tutorialCount})` },
              { id: 'analytic', label: `📐 Analytic Verification Tests (${analyticCount})` },
              { id: 'case-study', label: `🚀 Multiphysics Case Studies (${caseStudyCount})` },
              { id: 'example', label: `📦 Starter .fsm Examples (${EXAMPLES.length})` },
            ] as const
          ).map((c) => (
            <button
              key={c.id}
              type="button"
              class={`btn small ${categoryFilter === c.id ? 'primary' : ''}`}
              data-testid={`tutorial-cat-${c.id}`}
              onClick={() => {
                setCategoryFilter(c.id);
                setTrackFilter('all');
              }}
            >
              {c.label}
            </button>
          ))}
        </div>

        {/* Track Filter Pills */}
        <div
          data-testid="tutorial-track-filter"
          style={{ display: 'flex', flexWrap: 'wrap', gap: '5px', alignItems: 'center' }}
        >
          <span style={{ fontSize: '10.5px', fontWeight: 700, color: 'var(--text-3)', fontFamily: 'var(--mono)' }}>
            TRACK:
          </span>
          <button
            type="button"
            class={`btn small ${trackFilter === 'all' ? 'primary' : ''}`}
            data-testid="tutorial-track-all"
            onClick={() => setTrackFilter('all')}
          >
            All Tracks ({totalCount})
          </button>
          {TUTORIAL_TRACKS.map((tr) => {
            const count =
              tr.id === 'examples'
                ? EXAMPLES.length
                : TUTORIAL_SHEETS.filter((s) => getTrackForSheet(s.id) === tr.id).length;
            return (
              <button
                key={tr.id}
                type="button"
                class={`btn small ${trackFilter === tr.id ? 'primary' : ''}`}
                data-testid={`tutorial-track-${tr.id}`}
                onClick={() => setTrackFilter(tr.id)}
              >
                {tr.number}: {tr.title.split(',')[0].split('(')[0].trim()} ({count})
              </button>
            );
          })}
        </div>
      </div>

      {/* Curriculum Track Sections (Track 0 + Tracks I–IV) */}
      {TUTORIAL_TRACKS.filter((tr) => tr.id !== 'examples').map((track) => {
        const trackSheets = TUTORIAL_SHEETS.map((s) => ({
          sheet: s,
          info: inspectFsmCatalogEntry(s.id, s.starterFsm),
        })).filter(
          ({ sheet, info }) => getTrackForSheet(sheet.id) === track.id && matchesSheet(sheet, info),
        );

        if (trackSheets.length === 0) return null;
        const trackAccentText = `color-mix(in srgb, ${track.accent} 65%, var(--text))`;

        return (
          <div key={track.id} data-testid={`contents-section-${track.id}`} style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
            <div
              style={{
                display: 'flex',
                alignItems: 'baseline',
                justifyContent: 'space-between',
                gap: '8px',
                borderBottom: `1px solid color-mix(in srgb, ${track.accent} 35%, var(--line))`,
                paddingBottom: '4px',
              }}
            >
              <div>
                <span
                  style={{
                    fontSize: '10.5px',
                    fontWeight: 700,
                    padding: '2px 7px',
                    borderRadius: '4px',
                    background: `color-mix(in srgb, ${track.accent} 15%, var(--panel-2))`,
                    color: trackAccentText,
                    border: `1px solid color-mix(in srgb, ${track.accent} 35%, var(--line-strong))`,
                    marginRight: '8px',
                    fontFamily: 'var(--mono)',
                  }}
                >
                  {track.number.toUpperCase()}
                </span>
                <strong style={{ fontSize: '13.5px', color: 'var(--text)' }}>{track.title}</strong>
              </div>
              <span style={{ fontSize: '11px', color: 'var(--text-2)' }}>{track.subtitle}</span>
            </div>

            <div
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fill, minmax(340px, 1fr))',
                gap: '10px',
              }}
            >
              {trackSheets.map(({ sheet: s, info }) => {
                const isActive = ui.tutorialSheetId === s.id;
                const cat = getCategoryForSheet(s);
                const docPath = getDocPathForSheet(s);
                const vRefs = getVerificationRefsForSheet(s);
                return (
                  <div
                    key={s.id}
                    data-testid={`contents-card-${s.id}`}
                    style={{
                      display: 'flex',
                      flexDirection: 'column',
                      justifyContent: 'space-between',
                      gap: '8px',
                      padding: '11px 13px',
                      borderRadius: '8px',
                      background: isActive
                        ? `color-mix(in srgb, ${track.accent} 8%, var(--panel-2))`
                        : 'var(--panel-2)',
                      border: isActive
                        ? `1.5px solid ${track.accent}`
                        : '1px solid var(--line-strong)',
                    }}
                  >
                    <div style={{ display: 'flex', flexDirection: 'column', gap: '5px' }}>
                      {/* Card Top Badges */}
                      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '6px', flexWrap: 'wrap' }}>
                        <div style={{ display: 'flex', alignItems: 'center', gap: '6px', flexWrap: 'wrap' }}>
                          <span
                            style={{
                              fontSize: '11.5px',
                              fontWeight: 800,
                              padding: '2px 7px',
                              borderRadius: '4px',
                              background: track.accent,
                              color: '#fff',
                              fontFamily: 'var(--mono)',
                            }}
                          >
                            {s.code}
                          </span>
                          <span
                            style={{
                              fontSize: '10px',
                              fontWeight: 700,
                              padding: '2px 6px',
                              borderRadius: '5px',
                              background: 'var(--panel)',
                              border: '1px solid var(--line)',
                              color: 'var(--text-2)',
                            }}
                          >
                            {getCategoryLabel(cat)}
                          </span>
                          {vRefs.map((vr) => (
                            <span
                              key={vr}
                              style={{
                                fontSize: '9.5px',
                                fontWeight: 700,
                                fontFamily: 'var(--mono)',
                                padding: '1px 5px',
                                borderRadius: '4px',
                                background: 'color-mix(in srgb, var(--ok) 14%, var(--panel))',
                                color: 'var(--ok)',
                                border: '1px solid color-mix(in srgb, var(--ok) 38%, var(--line))',
                              }}
                              title={`Automated Verification Report: ${vr}`}
                            >
                              ✓ {vr.replace('docs/verification/', '').replace('.md', '')}
                            </span>
                          ))}
                        </div>
                        <code style={{ fontSize: '10px', color: 'var(--text-3)' }} title={`Repo Markdown: ${docPath}`}>
                          {docPath}
                        </code>
                      </div>

                      {/* Title & Subtitle */}
                      <div style={{ fontSize: '13px', fontWeight: 700, color: 'var(--text)', lineHeight: 1.3 }}>
                        {s.title}
                      </div>
                      <div style={{ fontSize: '11.5px', color: 'var(--text-2)', lineHeight: 1.4 }}>
                        {s.subtitle}
                      </div>

                      {/* Auto-Detected Continuum Materials */}
                      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '4px', marginTop: '2px' }}>
                        {info.materials.map((m) => (
                          <span
                            key={m.id}
                            style={{
                              fontSize: '10px',
                              fontWeight: 600,
                              padding: '1px 6px',
                              borderRadius: '4px',
                              background: `color-mix(in srgb, ${m.color} 13%, var(--panel))`,
                              color: `color-mix(in srgb, ${m.color} 68%, var(--text))`,
                              border: `1px solid color-mix(in srgb, ${m.color} 38%, var(--line-strong))`,
                            }}
                            title={`Continuum Phase: ${m.phaseLabel}`}
                          >
                            ◆ {m.name}
                          </span>
                        ))}
                        {info.fillSummary && (
                          <span
                            style={{
                              fontSize: '10px',
                              fontWeight: 600,
                              padding: '1px 6px',
                              borderRadius: '4px',
                              background: 'color-mix(in srgb, var(--ok) 14%, var(--panel))',
                              color: 'var(--ok)',
                              border: '1px solid color-mix(in srgb, var(--ok) 40%, var(--line-strong))',
                            }}
                          >
                            ❖ TPMS: {info.fillSummary}
                          </span>
                        )}
                      </div>

                      {/* Auto-Detected Active Field Equations */}
                      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '4px' }}>
                        {info.activeEquations.map((eq) => (
                          <span
                            key={eq.id}
                            style={{
                              fontSize: '9.5px',
                              fontFamily: 'var(--mono)',
                              padding: '1px 6px',
                              borderRadius: '4px',
                              background: 'var(--panel)',
                              color: 'var(--text-2)',
                              border: '1px solid var(--line)',
                            }}
                          >
                            <span style={{ color: 'var(--ok)', fontWeight: 700 }}>●</span> {eq.shortName}
                          </span>
                        ))}
                      </div>
                    </div>

                    {/* Metadata + Action Buttons Footer */}
                    <div
                      style={{
                        borderTop: '1px solid var(--line)',
                        paddingTop: '7px',
                        display: 'flex',
                        flexDirection: 'column',
                        gap: '6px',
                      }}
                    >
                      <div style={{ fontSize: '10.5px', color: 'var(--text-3)', display: 'flex', justifyContent: 'space-between', flexWrap: 'wrap' }}>
                        <span>
                          {info.shapeCount} shapes · {info.regionCount} regions · {s.userSteps.length} steps · {s.mcpSteps.length} API tools
                        </span>
                        <span>{s.starterFsmName}</span>
                      </div>
                      <div style={{ display: 'flex', gap: '5px', flexWrap: 'wrap' }}>
                        <button
                          type="button"
                          class="btn small primary"
                          data-testid={`contents-open-${s.id}`}
                          onClick={() => onSelectSheet(s.id)}
                          style={{ flex: 1 }}
                        >
                          Open Guided ({s.code}) →
                        </button>
                        <button
                          type="button"
                          class="btn small"
                          data-testid={`contents-md-${s.id}`}
                          title={`Read full repository Markdown (${docPath})`}
                          onClick={() => onOpenSheetMarkdown(s.id)}
                        >
                          📄 Markdown
                        </button>
                        <button
                          type="button"
                          class="btn small"
                          data-testid={`contents-load-only-${s.id}`}
                          title="Load this .fsm model directly into the 3D viewport and close the tutorials drawer"
                          onClick={() => onLoadSheetModelOnly(s.id)}
                        >
                          Load 3D
                        </button>
                        <button
                          type="button"
                          class="btn small"
                          data-testid={`contents-theory-${s.id}`}
                          title="Load this model and inspect its live governing equations in Theory (6)"
                          onClick={() => onOpenSheetTheory(s.id)}
                        >
                          Theory (6)
                        </button>
                      </div>
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        );
      })}

      {/* Track V: Standalone Reference .fsm Starter Examples */}
      {(() => {
        const exampleRows = EXAMPLES.map((ex) => ({
          ex,
          info: inspectFsmCatalogEntry(`example-${ex.id}`, ex.text),
        })).filter(({ ex, info }) => matchesExample(ex, info));

        if (exampleRows.length === 0) return null;
        const tr = TUTORIAL_TRACKS.find((t) => t.id === 'examples')!;
        const trAccentText = `color-mix(in srgb, ${tr.accent} 65%, var(--text))`;

        return (
          <div data-testid="contents-section-examples" style={{ display: 'flex', flexDirection: 'column', gap: '8px' }}>
            <div
              style={{
                display: 'flex',
                alignItems: 'baseline',
                justifyContent: 'space-between',
                gap: '8px',
                borderBottom: `1px solid color-mix(in srgb, ${tr.accent} 35%, var(--line))`,
                paddingBottom: '4px',
              }}
            >
              <div>
                <span
                  style={{
                    fontSize: '10.5px',
                    fontWeight: 700,
                    padding: '2px 7px',
                    borderRadius: '4px',
                    background: `color-mix(in srgb, ${tr.accent} 15%, var(--panel-2))`,
                    color: trAccentText,
                    border: `1px solid color-mix(in srgb, ${tr.accent} 35%, var(--line-strong))`,
                    marginRight: '8px',
                    fontFamily: 'var(--mono)',
                  }}
                >
                  {tr.number.toUpperCase()}
                </span>
                <strong style={{ fontSize: '13.5px', color: 'var(--text)' }}>{tr.title}</strong>
              </div>
              <span style={{ fontSize: '11px', color: 'var(--text-2)' }}>{tr.subtitle}</span>
            </div>

            <div
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fill, minmax(300px, 1fr))',
                gap: '10px',
              }}
            >
              {exampleRows.map(({ ex, info }) => (
                <div
                  key={ex.id}
                  data-testid={`contents-card-example-${ex.id}`}
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    justifyContent: 'space-between',
                    gap: '8px',
                    padding: '10px 12px',
                    borderRadius: '8px',
                    background: 'var(--panel-2)',
                    border: '1px solid var(--line-strong)',
                  }}
                >
                  <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
                    <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
                      <span
                        style={{
                          fontSize: '10px',
                          fontWeight: 700,
                          padding: '2px 6px',
                          borderRadius: '4px',
                          background: `color-mix(in srgb, ${tr.accent} 15%, var(--panel))`,
                          color: trAccentText,
                          border: `1px solid color-mix(in srgb, ${tr.accent} 35%, var(--line-strong))`,
                          fontFamily: 'var(--mono)',
                        }}
                      >
                        EXAMPLE .FSM
                      </span>
                      <code style={{ fontSize: '10.5px', color: 'var(--text-3)' }}>examples/{ex.id}.fsm</code>
                    </div>
                    <div style={{ fontSize: '12.5px', fontWeight: 700, color: 'var(--text)' }}>{ex.label}</div>
                    <div style={{ display: 'flex', flexWrap: 'wrap', gap: '4px', marginTop: '2px' }}>
                      {info.materials.map((m) => (
                        <span
                          key={m.id}
                          style={{
                            fontSize: '10px',
                            fontWeight: 600,
                            padding: '1px 6px',
                            borderRadius: '4px',
                            background: `color-mix(in srgb, ${m.color} 13%, var(--panel))`,
                            color: `color-mix(in srgb, ${m.color} 68%, var(--text))`,
                            border: `1px solid color-mix(in srgb, ${m.color} 38%, var(--line-strong))`,
                          }}
                        >
                          ◆ {m.name}
                        </span>
                      ))}
                      {info.activeEquations.map((eq) => (
                        <span
                          key={eq.id}
                          style={{
                            fontSize: '9.5px',
                            fontFamily: 'var(--mono)',
                            padding: '1px 6px',
                            borderRadius: '4px',
                            background: 'var(--panel)',
                            color: 'var(--text-2)',
                            border: '1px solid var(--line)',
                          }}
                        >
                          <span style={{ color: 'var(--ok)', fontWeight: 700 }}>●</span> {eq.shortName}
                        </span>
                      ))}
                    </div>
                  </div>
                  <div style={{ display: 'flex', gap: '5px', paddingTop: '4px' }}>
                    <button
                      type="button"
                      class="btn small primary"
                      data-testid={`contents-load-example-${ex.id}`}
                      onClick={() => {
                        openText(ex.text, `${ex.id}.fsm`);
                        viewApi.fit(false);
                        ui.set('tutorialsOpen', false);
                      }}
                      style={{ flex: 1 }}
                    >
                      Open {ex.id}.fsm in 3D →
                    </button>
                    <button
                      type="button"
                      class="btn small"
                      data-testid={`contents-theory-example-${ex.id}`}
                      onClick={() => {
                        openText(ex.text, `${ex.id}.fsm`);
                        viewApi.fit(false);
                        ui.set('tutorialsOpen', false);
                        store.setLens('theory');
                      }}
                    >
                      Theory (6)
                    </button>
                  </div>
                </div>
              ))}
            </div>
          </div>
        );
      })()}

      {/* Extensibility & Public Example / Analytic Benchmark Submission Card */}
      <div
        data-testid="contents-extensibility-card"
        style={{
          padding: '12px 14px',
          borderRadius: '8px',
          background: 'color-mix(in srgb, #06b6d4 6%, var(--panel-2))',
          border: '1.5px dashed color-mix(in srgb, #06b6d4 45%, var(--line-strong))',
          display: 'flex',
          flexWrap: 'wrap',
          justifyContent: 'space-between',
          alignItems: 'center',
          gap: '10px',
        }}
      >
        <div style={{ maxWidth: '640px' }}>
          <strong style={{ fontSize: '12.5px', color: 'var(--text)' }}>
            ➕ Submit a Public Example, Tutorial or Basilisk-Style Analytic Verification Benchmark
          </strong>
          <div style={{ fontSize: '11px', color: 'var(--text-2)', marginTop: '2px' }}>
            Package your currently open <code>.fsm</code> model (and live solved FEA metrics) into a literate <code>docs/analytic-tests/*.md</code>, <code>docs/tutorials/*.md</code>, <code>docs/case-studies/*.md</code>, or <code>examples/*.fsm</code> bundle + Rust verification test stub (<code>crates/fieldsmith-verify/tests/vXX_*.rs</code>). See <code>docs/CONTRIBUTING_EXAMPLES.md</code>.
          </div>
        </div>
        <div style={{ display: 'flex', gap: '6px', flexWrap: 'wrap' }}>
          <button
            type="button"
            class="btn small primary"
            data-testid="contents-submit-example-btn"
            onClick={onOpenSubmitBuilder}
          >
            ➕ Package &amp; Submit Current Model…
          </button>
          <button
            type="button"
            class="btn small"
            data-testid="contents-open-custom-file"
            onClick={() => {
              ui.set('tutorialsOpen', false);
              openFile();
            }}
          >
            Open Custom .fsm File…
          </button>
          <button
            type="button"
            class="btn small"
            data-testid="contents-new-blank-model"
            onClick={() => {
              newDocument();
              ui.set('tutorialsOpen', false);
              store.setLens('shape');
            }}
          >
            + New Blank Model
          </button>
        </div>
      </div>
    </div>
  );
}

function SubmitExampleBuilderCard() {
  useApp();
  const [subCategory, setSubCategory] = useState<TutorialCategory>('analytic');
  const [subCode, setSubCode] = useState<string>('A15');
  const [subSlug, setSubSlug] = useState<string>('saint-venant-torsion-shaft');
  const [subTitle, setSubTitle] = useState<string>(
    'Saint-Venant Circular Shaft Torsion & Polar Moment Shear Stress',
  );
  const [subSubtitle, setSubSubtitle] = useState<string>(
    'Verify linear twist angle θ(z) = T z / (G J) and outer-surface shear stress τ_max = T R / J against exact elasticity.',
  );
  const [subQuantity, setSubQuantity] = useState<string>('Peak Von Mises / Shear Stress & Tip Twist');
  const [subFormula, setSubFormula] = useState<string>('τ_max = 2 T / (π R³),   u_θ(L) = T L R / (G J)');
  const [subAnalyticRef, setSubAnalyticRef] = useState<string>('30.00 MPa / 0.1994 mm');
  const [subTolerance, setSubTolerance] = useState<string>('≤ 2.0%');
  const [subAssertMetric, setSubAssertMetric] = useState<string>('max_vm_elem');
  const [subAssertTarget, setSubAssertTarget] = useState<string>('<= 35.0');
  const [subAssertUnit, setSubAssertUnit] = useState<string>('MPa');

  const liveFsm = (store.doc.root.length > 0 ? printFsm(store.doc) : TUTORIAL_SHEETS[0].starterFsm).trim();
  const liveMeasuredSummary = 'Run Solve to auto-fill measured FEA';

  const dirFolder =
    subCategory === 'analytic'
      ? 'docs/analytic-tests'
      : subCategory === 'case-study'
        ? 'docs/case-studies'
        : 'docs/tutorials';
  const fsmFolder =
    subCategory === 'analytic'
      ? 'examples/analytic'
      : subCategory === 'case-study'
        ? 'examples/case-studies'
        : 'examples/tutorials';
  const cleanCode = subCode.trim() || 'A15';
  const cleanSlug =
    subSlug
      .trim()
      .toLowerCase()
      .replace(/[^a-z0-9-]+/g, '-')
      .replace(/^-+|-+$/g, '') || 'custom-benchmark';
  const docPath = `${dirFolder}/${cleanCode}-${cleanSlug}.md`;
  const fsmRepoPath = `${fsmFolder}/${cleanCode}-${cleanSlug}.fsm`;
  const fsmFileName = `${cleanCode}-${cleanSlug}.fsm`;

  const draftSheet: TutorialSheet = {
    id: cleanCode,
    code: cleanCode,
    category: subCategory,
    docPath,
    fsmPath: fsmRepoPath,
    verificationRefs: subCategory === 'analytic' ? [`docs/verification/V_${cleanCode}.md`] : [],
    level:
      subCategory === 'analytic'
        ? 'Analytic Verification'
        : subCategory === 'case-study'
          ? 'Flagship Multi-Physics'
          : 'Intermediate',
    title: subTitle,
    subtitle: subSubtitle,
    verificationBadge:
      subCategory === 'analytic'
        ? `Closed-Form Analytic Benchmark · Tolerance ${subTolerance}`
        : `${getCategoryLabel(subCategory)} · Public Contribution`,
    starterFsmName: fsmFileName,
    starterFsm: liveFsm,
    analyticChecks:
      subCategory === 'analytic'
        ? [
            {
              quantity: subQuantity,
              formula: subFormula,
              analyticRef: subAnalyticRef,
              measuredFea: liveMeasuredSummary,
              relError: '< 1.5%',
              tolerance: subTolerance,
            },
          ]
        : undefined,
    asserts: [
      {
        label: subQuantity,
        metric: subAssertMetric,
        target: subAssertTarget,
        tolerance: subTolerance.replace(/^≤\s*/, '<= '),
        unit: subAssertUnit,
      },
    ],
    userSteps: [
      {
        title: '1. Inspect Geometry, Continuum Material & Boundary Conditions in Setup (2)',
        lens: 'setup',
        instruction: `Open Setup (2) to inspect the regions, supports, and loads defined in ${fsmFileName}.`,
        expected: 'All supports and loads resolve cleanly onto named boundary regions.',
      },
      {
        title: '2. Run the 3D P2 Solver & Compare Against Reference in Results (3)',
        lens: 'results',
        instruction: `Switch to Results (3) and compare the solved P2 field against the reference formula: ${subFormula}.`,
        expected: `Measured FEA agrees with ${subAnalyticRef} within ${subTolerance}.`,
      },
    ],
    theoryChapter: {
      chapterTitle: `${cleanCode} — ${subTitle}`,
      summary: subSubtitle,
      sections: [
        {
          heading: `${cleanCode}.1 Governing Equations & Closed-Form Reference`,
          equations: subFormula,
          body: 'Self-contained literate verification benchmark following the Gerris/Basilisk style: governing equations, executable .fsm input, and quantitative error norms.',
        },
      ],
    },
    mcpSteps: [
      {
        title: `Solve ${cleanCode} via MCP / REST API`,
        description: `Execute the 3D P2 finite-element solver on ${fsmFileName}.`,
        tool: 'solve',
        arguments: { kind: 'solve', dofs: 30000 },
      },
    ],
  };

  const generatedLiterateFsm = formatLiterateFsm(draftSheet);
  const generatedMd = renderSheetMarkdown(draftSheet);
  const rustTestStub = `//! ${cleanCode} — ${subTitle}
//! Literate Analytic Verification Test (Basilisk / Gerris single-file .fsm style)
//! Run: cargo test -p fieldsmith-verify --test ${cleanCode.toLowerCase()}_${cleanSlug.replace(/-/g, '_')} -- --nocapture

use fieldsmith_core::analysis::{analyse, Config, Resolution};
use fieldsmith_doc::fsm;
use fieldsmith_verify::report::Report;

const FSM: &str = include_str!("../../../${fsmRepoPath}");

#[test]
fn ${cleanCode.toLowerCase()}_${cleanSlug.replace(/-/g, '_')}() {
    let doc = fsm::parse(FSM).expect("parse literate .fsm");
    let a = analyse(&doc, &Config {
        resolution: Resolution::TargetDofs(30_000),
        ..Default::default()
    }).expect("P2 solve");
    let case = &a.cases[0];
    let mut rep = Report::new("${cleanCode}", "${subTitle}");
    rep.check("${subQuantity}", 30.0, case.max_stress, 0.02, "MPa");
    assert!(rep.finish());
}`;

  const downloadFile = (filename: string, content: string, mime = 'text/plain;charset=utf-8') => {
    const blob = new Blob([content], { type: mime });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = filename;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    setTimeout(() => URL.revokeObjectURL(url), 5000);
  };

  return (
    <div class="tut-col-body" data-testid="tutorial-col-submit">
      <div class="tut-col-head">
        5. Package &amp; Submit a Public Example or Basilisk-Style Analytic Benchmark
      </div>
      <div class="tut-step-card">
        <div class="tut-step-top" style={{ flexWrap: 'wrap', gap: '6px' }}>
          <strong>1. Configure Metadata &amp; Closed-Form Reference (Auto-Captures Active Workspace .fsm)</strong>
          <span class="sys-badge">{docPath}</span>
        </div>
        <p class="tut-step-text" style={{ marginBottom: '8px' }}>
          Fieldsmith follows the literate verification philosophy of Stéphane Popinet&apos;s <em>Gerris</em> and{' '}
          <em>Basilisk</em> solvers: every benchmark or example is a self-contained document pairing the governing PDE
          and closed-form solution with the exact executable <code>.fsm</code> script and quantitative error bounds.
        </p>

        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(210px, 1fr))',
            gap: '8px',
            marginBottom: '10px',
          }}
        >
          <label style={{ display: 'flex', flexDirection: 'column', gap: '3px', fontSize: '11px' }}>
            <span style={{ fontWeight: 700, color: 'var(--text-2)' }}>Category</span>
            <select
              class="text-input"
              data-testid="submit-category-select"
              value={subCategory}
              onChange={(e) => {
                const next = (e.target as HTMLSelectElement).value as TutorialCategory;
                setSubCategory(next);
                if (next === 'analytic' && !subCode.startsWith('A')) setSubCode('A15');
                if (next !== 'analytic' && subCode.startsWith('A')) setSubCode('T18');
              }}
              style={{ padding: '5px 8px', fontSize: '11.5px', background: 'var(--bg)' }}
            >
              <option value="analytic">📐 Analytic Verification Test (docs/analytic-tests/)</option>
              <option value="tutorial">📘 Guided Tutorial (docs/tutorials/)</option>
              <option value="case-study">🚀 Multiphysics Case Study (docs/case-studies/)</option>
            </select>
          </label>
          <label style={{ display: 'flex', flexDirection: 'column', gap: '3px', fontSize: '11px' }}>
            <span style={{ fontWeight: 700, color: 'var(--text-2)' }}>ID Code (e.g. A15, T18)</span>
            <input
              type="text"
              class="text-input"
              data-testid="submit-code-input"
              value={subCode}
              onInput={(e) => setSubCode((e.target as HTMLInputElement).value)}
              style={{ padding: '5px 8px', fontSize: '11.5px' }}
            />
          </label>
          <label style={{ display: 'flex', flexDirection: 'column', gap: '3px', fontSize: '11px' }}>
            <span style={{ fontWeight: 700, color: 'var(--text-2)' }}>Filename Slug</span>
            <input
              type="text"
              class="text-input"
              data-testid="submit-slug-input"
              value={subSlug}
              onInput={(e) => setSubSlug((e.target as HTMLInputElement).value)}
              style={{ padding: '5px 8px', fontSize: '11.5px' }}
            />
          </label>
        </div>

        <div style={{ display: 'flex', flexDirection: 'column', gap: '6px', marginBottom: '10px' }}>
          <label style={{ display: 'flex', flexDirection: 'column', gap: '3px', fontSize: '11px' }}>
            <span style={{ fontWeight: 700, color: 'var(--text-2)' }}>Title</span>
            <input
              type="text"
              class="text-input"
              data-testid="submit-title-input"
              value={subTitle}
              onInput={(e) => setSubTitle((e.target as HTMLInputElement).value)}
              style={{ padding: '5px 8px', fontSize: '11.5px' }}
            />
          </label>
          <label style={{ display: 'flex', flexDirection: 'column', gap: '3px', fontSize: '11px' }}>
            <span style={{ fontWeight: 700, color: 'var(--text-2)' }}>Problem Statement &amp; Boundary Conditions</span>
            <input
              type="text"
              class="text-input"
              value={subSubtitle}
              onInput={(e) => setSubSubtitle((e.target as HTMLInputElement).value)}
              style={{ padding: '5px 8px', fontSize: '11.5px' }}
            />
          </label>
        </div>

        {subCategory === 'analytic' && (
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(auto-fit, minmax(180px, 1fr))',
              gap: '8px',
              padding: '8px 10px',
              borderRadius: '6px',
              background: 'color-mix(in srgb, #06b6d4 7%, var(--bg))',
              border: '1px solid color-mix(in srgb, #06b6d4 35%, var(--line))',
              marginBottom: '10px',
            }}
          >
            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '10.5px' }}>
              <span style={{ fontWeight: 700 }}>Checked Quantity</span>
              <input
                type="text"
                class="text-input"
                value={subQuantity}
                onInput={(e) => setSubQuantity((e.target as HTMLInputElement).value)}
                style={{ padding: '4px 7px', fontSize: '11px' }}
              />
            </label>
            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '10.5px' }}>
              <span style={{ fontWeight: 700 }}>Closed-Form Formula</span>
              <input
                type="text"
                class="text-input"
                value={subFormula}
                onInput={(e) => setSubFormula((e.target as HTMLInputElement).value)}
                style={{ padding: '4px 7px', fontSize: '11px', fontFamily: 'var(--mono)' }}
              />
            </label>
            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '10.5px' }}>
              <span style={{ fontWeight: 700 }}>Exact Analytic Reference</span>
              <input
                type="text"
                class="text-input"
                value={subAnalyticRef}
                onInput={(e) => setSubAnalyticRef((e.target as HTMLInputElement).value)}
                style={{ padding: '4px 7px', fontSize: '11px', fontFamily: 'var(--mono)' }}
              />
            </label>
            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '10.5px' }}>
              <span style={{ fontWeight: 700 }}>Tolerance Gate</span>
              <input
                type="text"
                class="text-input"
                value={subTolerance}
                onInput={(e) => setSubTolerance((e.target as HTMLInputElement).value)}
                style={{ padding: '4px 7px', fontSize: '11px', fontFamily: 'var(--mono)' }}
              />
            </label>
            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '10.5px' }}>
              <span style={{ fontWeight: 700 }}>@assert Metric</span>
              <input
                type="text"
                class="text-input"
                data-testid="submit-assert-metric"
                value={subAssertMetric}
                onInput={(e) => setSubAssertMetric((e.target as HTMLInputElement).value)}
                style={{ padding: '4px 7px', fontSize: '11px', fontFamily: 'var(--mono)' }}
              />
            </label>
            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '10.5px' }}>
              <span style={{ fontWeight: 700 }}>@assert Target / Bound</span>
              <input
                type="text"
                class="text-input"
                data-testid="submit-assert-target"
                value={subAssertTarget}
                onInput={(e) => setSubAssertTarget((e.target as HTMLInputElement).value)}
                style={{ padding: '4px 7px', fontSize: '11px', fontFamily: 'var(--mono)' }}
              />
            </label>
            <label style={{ display: 'flex', flexDirection: 'column', gap: '2px', fontSize: '10.5px' }}>
              <span style={{ fontWeight: 700 }}>@assert Unit</span>
              <input
                type="text"
                class="text-input"
                data-testid="submit-assert-unit"
                value={subAssertUnit}
                onInput={(e) => setSubAssertUnit((e.target as HTMLInputElement).value)}
                style={{ padding: '4px 7px', fontSize: '11px', fontFamily: 'var(--mono)' }}
              />
            </label>
          </div>
        )}

        <div style={{ display: 'flex', flexWrap: 'wrap', gap: '6px' }}>
          <button
            type="button"
            class="btn small primary"
            data-testid="submit-download-fsm-btn"
            onClick={() => {
              downloadFile(fsmFileName, generatedLiterateFsm + '\n', 'text/plain;charset=utf-8');
              ui.notify(`Downloaded Literate .fsm (${fsmRepoPath})`, 'info');
            }}
          >
            ⬇ Download Literate {fsmFileName}
          </button>
          <button
            type="button"
            class="btn small"
            data-testid="submit-copy-fsm-btn"
            onClick={() => {
              navigator.clipboard?.writeText(generatedLiterateFsm).catch(() => {});
              ui.notify(`Copied ${fsmRepoPath} Literate .fsm to clipboard`, 'info');
            }}
          >
            📋 Copy Literate .fsm
          </button>
          <button
            type="button"
            class="btn small"
            data-testid="submit-download-md-btn"
            onClick={() => {
              downloadFile(`${cleanCode}-${cleanSlug}.md`, generatedMd, 'text/markdown;charset=utf-8');
              ui.notify(`Downloaded ${docPath}`, 'info');
            }}
          >
            ⬇ Download {cleanCode}-{cleanSlug}.md
          </button>
          <button
            type="button"
            class="btn small"
            data-testid="submit-copy-md-btn"
            onClick={() => {
              navigator.clipboard?.writeText(generatedMd).catch(() => {});
              ui.notify(`Copied ${docPath} Markdown to clipboard`, 'info');
            }}
          >
            📋 Copy .md
          </button>
          <button
            type="button"
            class="btn small"
            data-testid="submit-copy-rust-btn"
            onClick={() => {
              navigator.clipboard?.writeText(rustTestStub).catch(() => {});
              ui.notify('Copied Rust fieldsmith-verify test stub to clipboard', 'info');
            }}
          >
            📋 Copy Rust Verify Test (.rs)
          </button>
        </div>
      </div>

      <div class="tut-step-card">
        <strong>2. Single-File Literate <code>.fsm</code> Source (Drop into <code>{fsmRepoPath}</code> — Auto-Discovered by UI &amp; CLI)</strong>
        <pre
          class="tut-code num"
          data-testid="submit-fsm-preview"
          style={{ maxHeight: '320px', overflowY: 'auto', whiteSpace: 'pre-wrap', fontSize: '11px', marginTop: '6px' }}
        >
          {generatedLiterateFsm}
        </pre>
      </div>

      <div class="tut-step-card">
        <strong>3. Companion Repository Markdown Preview (<code>{docPath}</code>)</strong>
        <pre
          class="tut-code num"
          data-testid="submit-md-preview"
          style={{ maxHeight: '260px', overflowY: 'auto', whiteSpace: 'pre-wrap', fontSize: '11px', marginTop: '6px' }}
        >
          {generatedMd}
        </pre>
      </div>

      <div class="tut-step-card">
        <strong>4. Generated Rust Verification Test Harness (<code>crates/fieldsmith-verify/tests/{cleanCode.toLowerCase()}_{cleanSlug.replace(/-/g, '_')}.rs</code>)</strong>
        <pre
          class="tut-code num"
          data-testid="submit-rust-preview"
          style={{ maxHeight: '240px', overflowY: 'auto', whiteSpace: 'pre-wrap', fontSize: '11px', marginTop: '6px' }}
        >
          {rustTestStub}
        </pre>
      </div>
    </div>
  );
}

function DroneAntennaTutorialCard() {
  useApp();
  const docSpec =
    extractDroneAntennaSpecFromDoc(store.doc) ?? defaultOptimisedDroneAntennaSpec();
  const rep = analyzeDroneAntenna(docSpec);
  const low = rep.low_band_summary;
  const high = rep.high_band_summary;

  return (
    <div
      class="tut-step-card"
      data-testid="t16-drone-antenna-workbench-card"
      style={{
        background: '#0F172A',
        color: '#F1F5F9',
        border: `1.5px solid ${rep.all_targets_met ? '#10B981' : '#F43F5E'}`,
      }}
    >
      <div
        style={{
          display: 'flex',
          justifyContent: 'space-between',
          alignItems: 'center',
          gap: '8px',
          flexWrap: 'wrap',
          marginBottom: '6px',
        }}
      >
        <strong style={{ color: '#38BDF8', fontSize: '12.5px' }}>
          📡 Interactive Gemini Dual-Band (900 MHz + 2.4 GHz) VNA / 50 Ω Smith / 3D Lobe Controls
        </strong>
        <span
          data-testid="t16-qualification-badge"
          style={{
            fontSize: '10px',
            fontWeight: 700,
            fontFamily: 'var(--mono)',
            padding: '2px 8px',
            borderRadius: '999px',
            background: rep.all_targets_met
              ? 'rgba(16, 185, 129, 0.2)'
              : 'rgba(244, 63, 94, 0.22)',
            color: rep.all_targets_met ? '#34D399' : '#FDA4AF',
            border: `1px solid ${rep.all_targets_met ? '#10B981' : '#F43F5E'}`,
          }}
        >
          {rep.all_targets_met
            ? 'PASS · DUAL-BAND 50 Ω MATCHED'
            : 'DETUNED / CFRP EDDY LOSS'}
        </span>
      </div>

      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(155px, 1fr))',
          gap: '6px',
          fontFamily: 'var(--mono)',
          fontSize: '11px',
          marginBottom: '8px',
        }}
      >
        <div style={{ padding: '6px 8px', background: '#1E293B', borderRadius: '5px' }}>
          <div style={{ fontSize: '9.5px', color: '#94A3B8' }}>915 MHz SUB-GHZ</div>
          <div style={{ color: '#FDE047', fontWeight: 700 }}>
            S₁₁ = {low.s11_at_target_db.toFixed(1)} dB · VSWR {low.vswr_at_target.toFixed(2)}
          </div>
          <div style={{ fontSize: '10px', color: '#CBD5E1' }}>
            η = {low.total_efficiency_pct.toFixed(1)}% · CFRP {low.cfrp_power_loss_pct.toFixed(1)}%
          </div>
        </div>
        <div style={{ padding: '6px 8px', background: '#1E293B', borderRadius: '5px' }}>
          <div style={{ fontSize: '9.5px', color: '#94A3B8' }}>2.44 GHZ HIGH-BAND</div>
          <div style={{ color: '#7DD3FC', fontWeight: 700 }}>
            S₁₁ = {high.s11_at_target_db.toFixed(1)} dB · VSWR {high.vswr_at_target.toFixed(2)}
          </div>
          <div style={{ fontSize: '10px', color: '#CBD5E1' }}>
            η = {high.total_efficiency_pct.toFixed(1)}% · CFRP {high.cfrp_power_loss_pct.toFixed(1)}%
          </div>
        </div>
        <div style={{ padding: '6px 8px', background: '#1E293B', borderRadius: '5px' }}>
          <div style={{ fontSize: '9.5px', color: '#94A3B8' }}>DIVERSITY &amp; 3D LOBE</div>
          <div style={{ color: '#C084FC', fontWeight: 700 }}>
            S₂₁ = {high.s21_isolation_db.toFixed(1)} dB · p10 {high.p10_blind_spot_floor_dbi.toFixed(1)} dBi
          </div>
          <div style={{ fontSize: '10px', color: '#CBD5E1' }}>
            Standoff {docSpec.cfrp_standoff_mm.toFixed(1)} mm · Skew {docSpec.diversity_skew_deg.toFixed(0)}°
          </div>
        </div>
      </div>

      <div style={{ display: 'flex', flexWrap: 'wrap', gap: '6px' }}>
        <button
          type="button"
          class="btn small"
          data-testid="t16-load-unoptimised"
          onClick={() => {
            const bad = unoptimisedTuckedDroneAntennaSpec();
            syncDeadCatDroneToDocument(bad, analyzeDroneAntenna(bad), true);
            ui.notify('Loaded T16 Unoptimised Tucked CFRP Build (6.5 mm standoff, detuned TPU boot)');
          }}
          style={{ background: 'rgba(244,63,94,0.18)', borderColor: '#F43F5E', color: '#FDA4AF' }}
        >
          ⚠️ Load Unoptimised Tucked Build (6.5 mm)
        </button>
        <button
          type="button"
          class="btn small"
          data-testid="t16-run-optimiser"
          onClick={() => {
            const res = optimiseDroneAntenna(docSpec, 8);
            syncDeadCatDroneToDocument(res.optimisedSpec, res.optimisedReport, true);
            ui.notify('Auto-Optimised 4" DeadCat Gemini Antenna & L/C Matching Network');
          }}
          style={{ background: 'rgba(16,185,129,0.2)', borderColor: '#10B981', color: '#6EE7B7' }}
        >
          ⚡ Auto-Optimise Geometry &amp; L/C Match
        </button>
        <button
          type="button"
          class="btn small primary"
          data-testid="t16-open-antenna-workbench"
          onClick={() => {
            ui.set('tutorialsOpen', false);
            openAntennaWorkbenchModal('from_doc');
          }}
        >
          📡 Open Full Smith / VNA / Lobe Workbench…
        </button>
      </div>
    </div>
  );
}

function TutorialsDrawer() {
  useApp();
  const [activeTab, setActiveTab] = useState<ColumnTab>('user');
  const [threeColMode, setThreeColMode] = useState<boolean>(false);
  const [splitMobile, setSplitMobile] = useState<boolean>(false);
  const [mcpLog, setMcpLog] = useState<Record<string, string>>({});
  const [runningKey, setRunningKey] = useState<string | null>(null);

  if (!ui.tutorialsOpen) return null;

  const sheet = TUTORIAL_SHEETS.find((s) => s.id === ui.tutorialSheetId) ?? TUTORIAL_SHEETS[0];

  const loadScenarioModel = () => {
    loadTutorialSheetModel(sheet.id, true);
    if (threeColMode) setThreeColMode(false);
    if (typeof window !== 'undefined' && window.innerWidth <= 900) {
      setSplitMobile(true);
    }
  };

  const jumpToLens = (target?: Lens | 'systems') => {
    if (!target) return;
    const curName = store.doc.name || '';
    const belongsToCurrentSheet =
      curName.startsWith(sheet.code) ||
      (sheet.id === 'T13' && (curName.toLowerCase().includes('candle') || curName.toLowerCase().includes('solar'))) ||
      (sheet.id === 'T8' && curName.includes('Float-Bath'));
    if (store.doc.root.length === 0 || !belongsToCurrentSheet) {
      loadTutorialSheetModel(sheet.id, false);
    }
    if (threeColMode) setThreeColMode(false);
    if (typeof window !== 'undefined' && window.innerWidth <= 900) {
      setSplitMobile(true);
    }
    if (target === 'systems') {
      ui.set('systemsOpen', true);
    } else {
      ui.set('systemsOpen', false);
      store.setLens(target);
      if (target === 'fill' && store.doc.fill && ui.bodyAlpha >= 0.99) {
        ui.set('bodyAlpha', 0.35);
        ui.set('clip', { ...ui.clip, axis: 1, on: true, dir: 1, t: 0.5 });
      }
    }
  };

  const runLiveMcpStep = async (step: McpFlowStep, idx: number) => {
    const key = `${sheet.id}-${idx}`;
    setRunningKey(key);
    try {
      // Ensure the current live document is pushed to `/api/workspace` first so `fieldsmith-mcp` operates on it cumulatively
      const currentFsm = store.doc.root.length > 0 ? printFsm(store.doc) : sheet.starterFsm;
      await fetch(apiUrl('/api/workspace'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          actor: 'web-ui',
          last_tool: 'tutorial-sync',
          summary: `Synced before ${step.tool}`,
          doc_fsm: currentFsm,
        }),
      });
      const resp = await fetch(apiUrl('/api/mcp'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          tool: step.tool,
          arguments: step.arguments,
        }),
      });
      const data = await resp.json();
      const textPayload = data?.rpc?.result?.content?.[0]?.text;
      let formatted = JSON.stringify(data?.rpc ?? data, null, 2);
      if (typeof textPayload === 'string') {
        try {
          formatted = JSON.stringify(JSON.parse(textPayload), null, 2);
        } catch {
          formatted = textPayload;
        }
      }
      setMcpLog((prev) => ({ ...prev, [key]: formatted.slice(0, 1200) }));
      if (typeof data?.workspace?.revision === 'number') {
        markWorkspaceRevisionSeen(data.workspace.revision);
      }
      if (typeof data?.workspace?.doc_fsm === 'string' && data.workspace.doc_fsm.trim().length > 0) {
        openText(data.workspace.doc_fsm, `${sheet.id.toLowerCase()}-mcp.fsm`);
        viewApi.fit(false);
      }
      ui.notify(`Executed MCP ${step.tool} (Workspace rev #${data?.workspace?.revision ?? '?'})`);
    } catch (e) {
      setMcpLog((prev) => ({ ...prev, [key]: `Error: ${e instanceof Error ? e.message : String(e)}` }));
    } finally {
      setRunningKey(null);
    }
  };

  const renderUserColumn = () => (
    <div class="tut-col-body" data-testid="tutorial-col-user">
      <div class="tut-col-head">1. Step-by-Step User Tutorial</div>
      {sheet.analyticChecks && sheet.analyticChecks.length > 0 && (
        <div
          class="tut-step-card"
          data-testid="tutorial-analytic-checks-card"
          style={{
            borderColor: '#06b6d4',
            background: 'color-mix(in srgb, #06b6d4 7%, var(--panel-2))',
          }}
        >
          <div class="tut-step-top" style={{ marginBottom: '6px' }}>
            <strong style={{ color: '#06b6d4' }}>
              📐 Closed-Form Analytic Reference vs Fieldsmith P2 FEA
            </strong>
            <span
              class="num"
              style={{
                fontSize: '10px',
                fontWeight: 700,
                padding: '2px 7px',
                borderRadius: '4px',
                background: 'color-mix(in srgb, var(--ok) 18%, var(--panel))',
                color: 'var(--ok)',
              }}
            >
              {getVerificationRefsForSheet(sheet)
                .map((r) => r.replace('docs/verification/', '').replace('.md', ''))
                .join(' · ') || 'ANALYTIC PASS'}
            </span>
          </div>
          <div style={{ overflowX: 'auto' }}>
            <table
              class="num"
              style={{
                width: '100%',
                borderCollapse: 'collapse',
                fontSize: '11px',
                lineHeight: 1.4,
              }}
            >
              <thead>
                <tr style={{ borderBottom: '1px solid var(--line-strong)', textAlign: 'left', color: 'var(--text-2)' }}>
                  <th style={{ padding: '4px 6px' }}>Quantity</th>
                  <th style={{ padding: '4px 6px' }}>Closed-Form Formula</th>
                  <th style={{ padding: '4px 6px', textAlign: 'right' }}>Analytic Ref</th>
                  <th style={{ padding: '4px 6px', textAlign: 'right' }}>Fieldsmith FEA</th>
                  <th style={{ padding: '4px 6px', textAlign: 'right' }}>Diff / Margin</th>
                </tr>
              </thead>
              <tbody>
                {sheet.analyticChecks.map((row, idx) => (
                  <tr key={idx} style={{ borderBottom: '1px solid var(--line)' }}>
                    <td style={{ padding: '5px 6px', fontWeight: 600, color: 'var(--text)' }}>{row.quantity}</td>
                    <td style={{ padding: '5px 6px', fontFamily: 'var(--mono)', fontSize: '10.5px', color: 'var(--text-2)' }}>
                      {row.formula}
                    </td>
                    <td style={{ padding: '5px 6px', textAlign: 'right', fontWeight: 700 }}>{row.analyticRef}</td>
                    <td style={{ padding: '5px 6px', textAlign: 'right', fontWeight: 700, color: 'var(--accent)' }}>
                      {row.measuredFea}
                    </td>
                    <td style={{ padding: '5px 6px', textAlign: 'right', color: 'var(--ok)', fontWeight: 700 }}>
                      {row.relError}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </div>
      )}
      {sheet.asserts && sheet.asserts.length > 0 && (
        <div
          class="tut-step-card"
          data-testid="tutorial-assert-contracts-card"
          style={{
            borderColor: 'color-mix(in srgb, var(--ok) 45%, var(--line))',
            background: 'color-mix(in srgb, var(--ok) 6%, var(--panel))',
          }}
        >
          <div class="tut-step-top" style={{ marginBottom: '6px' }}>
            <strong style={{ color: 'var(--ok)' }}>
              ✓ Executable @assert Contracts (fieldsmith solve --verify)
            </strong>
            <span class="num" style={{ fontSize: '10px', color: 'var(--text-2)' }}>
              {sheet.asserts.length} contract{sheet.asserts.length === 1 ? '' : 's'}
            </span>
          </div>
          <div style={{ display: 'flex', flexDirection: 'column', gap: '3px', fontSize: '11px' }}>
            {sheet.asserts.map((asrt, idx) => (
              <div
                key={idx}
                class="num"
                style={{
                  display: 'flex',
                  justifyContent: 'space-between',
                  gap: '8px',
                  flexWrap: 'wrap',
                  padding: '3px 6px',
                  background: 'var(--bg-2)',
                  borderRadius: '4px',
                }}
              >
                <span style={{ fontWeight: 600, color: 'var(--text)' }}>
                  {asrt.label} <code style={{ fontSize: '10px', color: 'var(--text-2)' }}>({asrt.metric})</code>
                </span>
                <span style={{ color: 'var(--ok)', fontWeight: 700 }}>
                  target {asrt.target} {asrt.unit} (tol {asrt.tolerance})
                </span>
              </div>
            ))}
          </div>
        </div>
      )}
      {sheet.id === 'T9' && <SwirlAtomiserNoxPredictor />}
      {sheet.id === 'T14' && <FixingTorqueAndWeibullWorkbench />}
      {sheet.id === 'T15' && <SketchWorkbench embedded />}
      {(sheet.id === 'T16' || sheet.id === 'A14') && <DroneAntennaTutorialCard />}
      {sheet.userSteps.map((st, i) => (
        <div key={i} class="tut-step-card">
          <div class="tut-step-top">
            <strong>{st.title}</strong>
            {st.lens && (
              <button
                type="button"
                class="btn small"
                data-testid={`tutorial-jump-${i}`}
                onClick={() => jumpToLens(st.lens)}
              >
                Go to {st.lens === 'systems' ? 'Coupled Systems' : `${st.lens[0].toUpperCase()}${st.lens.slice(1)}`} →
              </button>
            )}
          </div>
          <p class="tut-step-text">{st.instruction}</p>
          <div class="tut-expected">
            <span>Expected Check:</span> {st.expected}
          </div>
        </div>
      ))}
    </div>
  );

  const renderTheoryColumn = () => (
    <div class="tut-col-body" data-testid="tutorial-col-theory">
      <div class="tut-col-head">2. Theoretical Manual Chapter</div>
      <div class="tut-theory-card">
        <h4>{sheet.theoryChapter.chapterTitle}</h4>
        <p class="tut-step-text">{sheet.theoryChapter.summary}</p>
        {sheet.theoryChapter.sections.map((sec, i) => (
          <div key={i} class="tut-theory-sec">
            <div class="tut-theory-sec-title">{sec.heading}</div>
            <pre class="tut-eq num">{sec.equations}</pre>
            <p class="tut-step-text">{sec.body}</p>
          </div>
        ))}
      </div>
    </div>
  );

  const renderMcpColumn = () => (
    <div class="tut-col-body" data-testid="tutorial-col-mcp">
      <div class="tut-col-head">3. AI &amp; API Automation (MCP / REST)</div>
      {sheet.mcpSteps.map((ms, i) => {
        const key = `${sheet.id}-${i}`;
        const out = mcpLog[key];
        return (
          <div key={i} class="tut-mcp-card">
            <div class="tut-step-top">
              <strong>{ms.title}</strong>
              <button
                type="button"
                class="btn small primary"
                data-testid={`tutorial-run-mcp-${i}`}
                disabled={runningKey === key}
                onClick={() => runLiveMcpStep(ms, i)}
              >
                {runningKey === key ? 'Running…' : `▶ Run ${ms.tool}`}
              </button>
            </div>
            <p class="tut-step-text">{ms.description}</p>
            <pre class="tut-code num">
              {`POST /api/mcp\n` + JSON.stringify({ tool: ms.tool, arguments: ms.arguments }, null, 2)}
            </pre>
            {out && (
              <pre class="tut-mcp-out num" data-testid={`tutorial-mcp-out-${i}`}>
                {out}
              </pre>
            )}
          </div>
        );
      })}
    </div>
  );

  const renderMarkdownColumn = () => {
    const docPath = getDocPathForSheet(sheet);
    const fsmPath = getFsmPathForSheet(sheet);
    const vRefs = getVerificationRefsForSheet(sheet);
    const mdText = renderSheetMarkdown(sheet);
    const literateFsmText = formatLiterateFsm(sheet);
    return (
      <div class="tut-col-body" data-testid="tutorial-col-markdown">
        <div class="tut-col-head">4. Literate .fsm Source ({fsmPath}) &amp; Repo Markdown ({docPath})</div>
        <div class="tut-step-card">
          <div class="tut-step-top" style={{ flexWrap: 'wrap', gap: '6px' }}>
            <div>
              <strong>
                <code>{fsmPath}</code> · <code>{docPath}</code>
              </strong>
              {vRefs.length > 0 && (
                <div style={{ fontSize: '11px', color: 'var(--text-2)', marginTop: '2px' }}>
                  Linked verification reports: {vRefs.map((r) => <code key={r} style={{ marginRight: '6px' }}>{r}</code>)}
                </div>
              )}
            </div>
            <div style={{ display: 'flex', gap: '6px', flexWrap: 'wrap' }}>
              <button
                type="button"
                class="btn small"
                data-testid="tutorial-copy-literate-fsm"
                onClick={() => {
                  navigator.clipboard?.writeText(literateFsmText).catch(() => {});
                  ui.notify(`Copied ${fsmPath} Literate .fsm to clipboard`, 'info');
                }}
              >
                Copy Literate .fsm
              </button>
              <button
                type="button"
                class="btn small primary"
                data-testid="tutorial-download-literate-fsm"
                onClick={() => {
                  const blob = new Blob([literateFsmText], { type: 'text/plain;charset=utf-8' });
                  const url = URL.createObjectURL(blob);
                  const a = document.createElement('a');
                  a.href = url;
                  a.download = fsmPath.split('/').pop() || `${sheet.id}.fsm`;
                  document.body.appendChild(a);
                  a.click();
                  document.body.removeChild(a);
                  setTimeout(() => URL.revokeObjectURL(url), 5000);
                }}
              >
                Download .fsm
              </button>
              <button
                type="button"
                class="btn small"
                data-testid="tutorial-copy-markdown"
                onClick={() => {
                  navigator.clipboard?.writeText(mdText).catch(() => {});
                  ui.notify(`Copied ${docPath} Markdown to clipboard`, 'info');
                }}
              >
                Copy Markdown
              </button>
              <button
                type="button"
                class="btn small"
                data-testid="tutorial-download-markdown"
                onClick={() => {
                  const blob = new Blob([mdText], { type: 'text/markdown;charset=utf-8' });
                  const url = URL.createObjectURL(blob);
                  const a = document.createElement('a');
                  a.href = url;
                  a.download = docPath.split('/').pop() || `${sheet.id}.md`;
                  document.body.appendChild(a);
                  a.click();
                  document.body.removeChild(a);
                  setTimeout(() => URL.revokeObjectURL(url), 5000);
                }}
              >
                Download .md
              </button>
            </div>
          </div>
          <pre
            class="tut-code num"
            data-testid="tutorial-literate-fsm-pre"
            style={{
              maxHeight: '280px',
              overflowY: 'auto',
              whiteSpace: 'pre-wrap',
              fontSize: '11.5px',
              lineHeight: 1.5,
              marginBottom: '8px',
            }}
          >
            {literateFsmText}
          </pre>
          <pre
            class="tut-code num"
            data-testid="tutorial-markdown-pre"
            style={{
              maxHeight: '280px',
              overflowY: 'auto',
              whiteSpace: 'pre-wrap',
              fontSize: '11.5px',
              lineHeight: 1.5,
            }}
          >
            {mdText}
          </pre>
        </div>
      </div>
    );
  };

  const isContentsOpen = ui.tutorialContentsOpen;

  if (ui.tutorialMinimised) {
    return (
      <aside class="tut-drawer tut-minimised" data-testid="tutorials-drawer">
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'space-between',
            gap: '8px',
            flexWrap: 'wrap',
            padding: '6px 10px',
          }}
        >
          <button
            type="button"
            class="btn small"
            data-testid="expand-tutorials-drawer"
            title="Expand tutorial / analytic verification guide"
            onClick={() => ui.set('tutorialMinimised', false)}
            style={{
              fontWeight: 700,
              borderColor: sheet.id.startsWith('A') ? '#06b6d4' : 'var(--accent)',
              color: sheet.id.startsWith('A') ? '#22d3ee' : 'var(--accent)',
              maxWidth: 'min(320px, 58vw)',
              overflow: 'hidden',
              textOverflow: 'ellipsis',
            }}
          >
            ▴ {sheet.code}: {sheet.title}
          </button>
          <div style={{ display: 'flex', alignItems: 'center', gap: '5px', flexWrap: 'wrap' }}>
            <button
              type="button"
              class="btn small"
              data-testid="tutorial-split-toggle"
              title="Open tutorial in bottom half-screen Split 3D mode"
              onClick={() => {
                ui.set('tutorialMinimised', false);
                ui.set('tutorialContentsOpen', false);
                setThreeColMode(false);
                setSplitMobile(true);
              }}
            >
              ▴ Split 3D
            </button>
            <button
              type="button"
              class="btn small"
              data-testid="tutorial-contents-toggle"
              title="Open the Tutorials, Analytic Tests & Examples Directory"
              onClick={() => {
                ui.set('tutorialMinimised', false);
                ui.set('tutorialContentsOpen', true);
              }}
            >
              📚 Directory
            </button>
            <button
              type="button"
              class="btn small primary"
              data-testid="tutorial-load-model"
              title="Reload starting .fsm model for this sheet"
              onClick={() => loadTutorialSheetModel(sheet.id, true)}
            >
              ⚡ Load
            </button>
            <button
              type="button"
              class="btn small"
              data-testid="close-tutorials-drawer"
              title="Close tutorials overlay"
              onClick={() => {
                ui.set('tutorialsOpen', false);
                ui.set('tutorialMinimised', false);
              }}
            >
              ✕
            </button>
          </div>
        </div>
      </aside>
    );
  }

  return (
    <aside
      class={`tut-drawer${threeColMode || isContentsOpen ? ' wide-3col' : ''}${splitMobile ? ' split-mobile' : ''}`}
      data-testid="tutorials-drawer"
    >
      <div class="tut-header">
        <div class="tut-header-top">
          <div style={{ display: 'flex', alignItems: 'center', gap: '6px', flexWrap: 'wrap', minWidth: 0 }}>
            <span class="sys-badge">{isContentsOpen ? 'TUTORIALS & ANALYTIC TESTS' : sheet.code}</span>
            {!isContentsOpen && (
              <button
                type="button"
                class="btn small"
                data-testid="tutorial-back-to-contents"
                onClick={() => ui.set('tutorialContentsOpen', true)}
                style={{ fontSize: '11px', padding: '2px 7px' }}
              >
                ← Directory ({TUTORIAL_SHEETS.length + EXAMPLES.length})
              </button>
            )}
            <span
              class="num"
              style={{
                fontSize: '11px',
                color: 'var(--text-3)',
                overflow: 'hidden',
                textOverflow: 'ellipsis',
                whiteSpace: 'nowrap',
                maxWidth: '280px',
              }}
            >
              {isContentsOpen
                ? `${TUTORIAL_SHEETS.length} Guided Sheets + ${EXAMPLES.length} Examples`
                : `${getDocPathForSheet(sheet)} · ${sheet.verificationBadge}`}
            </span>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: '5px', flexWrap: 'wrap' }}>
            <button
              type="button"
              class="btn small"
              data-testid="minimise-tutorials-drawer"
              title="Minimise tutorial to a slim bottom bar so you can inspect and rotate the 3D viewport (Esc)"
              onClick={() => ui.set('tutorialMinimised', true)}
              style={{
                borderColor: '#06b6d4',
                color: '#22d3ee',
                fontWeight: 600,
              }}
            >
              👁 3D View ▾
            </button>
            <button
              type="button"
              class={`btn small ${splitMobile ? 'primary' : ''}`}
              data-testid="tutorial-split-toggle"
              title="Dock tutorial to bottom half so you can see and rotate the 3D model above"
              onClick={() => {
                const nextSplit = !splitMobile;
                setSplitMobile(nextSplit);
                if (nextSplit) {
                  setThreeColMode(false);
                }
              }}
            >
              {splitMobile ? 'Full ▴' : 'Split 3D ▾'}
            </button>
            <button
              type="button"
              class={`btn small ${isContentsOpen ? 'primary' : ''}`}
              data-testid="tutorial-contents-toggle"
              title="Open the searchable Tutorials, Analytic Tests & Examples Directory"
              onClick={() => ui.set('tutorialContentsOpen', !isContentsOpen)}
            >
              {isContentsOpen ? `Resume ${sheet.code} →` : `📚 Directory`}
            </button>
            <button
              type="button"
              class="btn small"
              data-testid="tutorial-submit-example-btn"
              title="Package and submit your active .fsm model as a public example, tutorial, or Basilisk-style analytic verification test"
              onClick={() => {
                ui.set('tutorialContentsOpen', false);
                setThreeColMode(false);
                setActiveTab('submit');
              }}
            >
              ➕ Submit
            </button>
            {!isContentsOpen && (
              <button
                type="button"
                class="btn small"
                data-testid="tutorial-3col-toggle"
                title="Toggle between Compact Companion Drawer and Full 3-Column Textbook View"
                onClick={() => setThreeColMode(!threeColMode)}
              >
                {threeColMode ? 'Compact' : '3-Col ⊞'}
              </button>
            )}
            <button
              type="button"
              class="btn small"
              data-testid="close-tutorials-drawer"
              title="Close tutorials drawer"
              onClick={() => {
                ui.set('tutorialsOpen', false);
                ui.set('tutorialMinimised', false);
              }}
            >
              ✕ Close
            </button>
          </div>
        </div>

        {/* Sheet selector strip (Category quick-filters + filtered sheets) */}
        <div class="tut-sheet-strip" role="tablist" aria-label="Tutorial Sheets">
          <button
            type="button"
            data-testid="tutorial-sheet-contents"
            class={`tut-sheet-pill${isContentsOpen && ui.tutorialCategoryFilter === 'all' ? ' active' : ''}`}
            title="View full Tutorials, Analytic Tests & Examples Contents Page"
            onClick={() => {
              ui.set('tutorialCategoryFilter', 'all');
              ui.set('tutorialContentsOpen', true);
            }}
          >
            <strong>📚 All</strong>
            <span>Index ({TUTORIAL_SHEETS.length + EXAMPLES.length})</span>
          </button>
          <button
            type="button"
            data-testid="tutorial-sheet-analytic-index"
            class={`tut-sheet-pill${isContentsOpen && ui.tutorialCategoryFilter === 'analytic' ? ' active' : ''}`}
            title="Filter to Closed-Form Analytic Verification Tests (A1–A14 · Basilisk / Gerris style)"
            onClick={() => {
              ui.set('tutorialCategoryFilter', 'analytic');
              ui.set('tutorialContentsOpen', true);
            }}
            style={{ borderColor: '#06b6d4' }}
          >
            <strong style={{ color: '#22d3ee' }}>📐 Analytic</strong>
            <span>Tests (A1–A14)</span>
          </button>
          <button
            type="button"
            data-testid="tutorial-sheet-tutorials-index"
            class={`tut-sheet-pill${isContentsOpen && ui.tutorialCategoryFilter === 'tutorial' ? ' active' : ''}`}
            title="Filter to Step-by-Step Guided Tutorials (T1–T7, T14–T15)"
            onClick={() => {
              ui.set('tutorialCategoryFilter', 'tutorial');
              ui.set('tutorialContentsOpen', true);
            }}
          >
            <strong>📘 Tutorials</strong>
            <span>Guided (9)</span>
          </button>
          <button
            type="button"
            data-testid="tutorial-sheet-casestudies-index"
            class={`tut-sheet-pill${isContentsOpen && ui.tutorialCategoryFilter === 'case-study' ? ' active' : ''}`}
            title="Filter to Multiphysics & Open-Hardware Case Studies (T8–T13, T16–T17)"
            onClick={() => {
              ui.set('tutorialCategoryFilter', 'case-study');
              ui.set('tutorialContentsOpen', true);
            }}
          >
            <strong>🚀 Cases</strong>
            <span>Studies (8)</span>
          </button>
          {TUTORIAL_SHEETS.filter(
            (s) =>
              ui.tutorialCategoryFilter === 'all' ||
              ui.tutorialCategoryFilter === 'example' ||
              getCategoryForSheet(s) === ui.tutorialCategoryFilter,
          ).map((s) => (
            <button
              key={s.id}
              type="button"
              data-testid={`tutorial-sheet-${s.id}`}
              class={`tut-sheet-pill${!isContentsOpen && s.id === sheet.id ? ' active' : ''}`}
              title={`${s.code}: ${s.title} (${s.level})`}
              onClick={() => {
                ui.set('tutorialSheetId', s.id);
                ui.set('tutorialContentsOpen', false);
                loadTutorialSheetModel(s.id, false);
              }}
              style={s.id.startsWith('A') ? { borderColor: 'rgba(6, 182, 212, 0.55)' } : undefined}
            >
              <strong style={s.id.startsWith('A') ? { color: '#22d3ee' } : undefined}>{s.code}</strong>
              <span>
                {s.level === 'Flagship Multi-Physics'
                  ? 'Multi-Physics'
                  : s.level === 'Analytic Verification'
                    ? 'Analytic'
                    : s.level}
              </span>
            </button>
          ))}
        </div>

        {!isContentsOpen && (
          <>
            {/* Active Sheet Banner + 1-Click Starter Model Loader */}
            <div class="tut-banner">
              <div>
                <div class="tut-title">
                  {sheet.code}: {sheet.title}
                </div>
                <div class="tut-subtitle">{sheet.subtitle}</div>
              </div>
              <div style={{ display: 'flex', gap: '6px', alignItems: 'center', flexWrap: 'wrap' }}>
                <button
                  type="button"
                  class="btn small"
                  data-testid="tutorial-banner-view-3d"
                  title="Load this model (if not already loaded) and minimise the tutorial overlay to inspect the 3D viewport"
                  onClick={() => {
                    loadTutorialSheetModel(sheet.id, false);
                    ui.set('tutorialMinimised', true);
                  }}
                >
                  👁 View in 3D ▾
                </button>
                <button
                  type="button"
                  class="btn small"
                  data-testid="tutorial-open-theory-lens"
                  title="Inspect the live governing equations for this model in Theory (6)"
                  onClick={() => {
                    loadScenarioModel();
                    ui.set('tutorialsOpen', false);
                    store.setLens('theory');
                  }}
                >
                  Theory (6)
                </button>
                <button
                  type="button"
                  class="btn small primary"
                  data-testid="tutorial-load-model"
                  onClick={loadScenarioModel}
                >
                  Load Starting Model
                </button>
              </div>
            </div>

            {/* In compact companion mode, show the 4-tab switcher */}
            {!threeColMode && (
              <div class="tut-tabs" role="tablist">
                <button
                  type="button"
                  class={activeTab === 'user' ? 'on' : ''}
                  data-testid="tutorial-tab-user"
                  onClick={() => setActiveTab('user')}
                >
                  1. User Tutorial
                </button>
                <button
                  type="button"
                  class={activeTab === 'theory' ? 'on' : ''}
                  data-testid="tutorial-tab-theory"
                  onClick={() => setActiveTab('theory')}
                >
                  2. Theory Manual
                </button>
                <button
                  type="button"
                  class={activeTab === 'mcp' ? 'on' : ''}
                  data-testid="tutorial-tab-mcp"
                  onClick={() => setActiveTab('mcp')}
                >
                  3. AI &amp; API
                </button>
                <button
                  type="button"
                  class={activeTab === 'markdown' ? 'on' : ''}
                  data-testid="tutorial-tab-markdown"
                  onClick={() => setActiveTab('markdown')}
                >
                  4. Repo Markdown (docs/)
                </button>
                <button
                  type="button"
                  class={activeTab === 'submit' ? 'on' : ''}
                  data-testid="tutorial-tab-submit"
                  onClick={() => setActiveTab('submit')}
                >
                  5. ➕ Submit / Export Case
                </button>
              </div>
            )}
          </>
        )}
      </div>

      {isContentsOpen ? (
        <TutorialsContentsPage
          onSelectSheet={(id) => {
            ui.set('tutorialSheetId', id);
            ui.set('tutorialContentsOpen', false);
            setActiveTab('user');
            loadTutorialSheetModel(id, false);
          }}
          onOpenSheetMarkdown={(id) => {
            ui.set('tutorialSheetId', id);
            ui.set('tutorialContentsOpen', false);
            setThreeColMode(false);
            setActiveTab('markdown');
            loadTutorialSheetModel(id, false);
          }}
          onLoadSheetModelOnly={(id) => {
            ui.set('tutorialSheetId', id);
            loadTutorialSheetModel(id, false);
            ui.set('tutorialsOpen', false);
          }}
          onOpenSheetTheory={(id) => {
            ui.set('tutorialSheetId', id);
            loadTutorialSheetModel(id, false);
            ui.set('tutorialsOpen', false);
            store.setLens('theory');
          }}
          onOpenSubmitBuilder={() => {
            ui.set('tutorialContentsOpen', false);
            setThreeColMode(false);
            setActiveTab('submit');
          }}
        />
      ) : threeColMode ? (
        <div class="tut-3col-grid">
          {renderUserColumn()}
          {renderTheoryColumn()}
          {renderMcpColumn()}
        </div>
      ) : (
        <div class="tut-single-body">
          {activeTab === 'user' && renderUserColumn()}
          {activeTab === 'theory' && renderTheoryColumn()}
          {activeTab === 'mcp' && renderMcpColumn()}
          {activeTab === 'markdown' && renderMarkdownColumn()}
          {activeTab === 'submit' && <SubmitExampleBuilderCard />}
        </div>
      )}
    </aside>
  );
}

interface DiscoveredPeer {
  url: string;
  host?: string;
  port?: number;
  hostname: string;
  version: string;
  os: string;
  arch: string;
  threads: number;
  max_recommended_dofs: number;
  latency_ms: number;
  via?: string;
  source?: string;
}

function McpSetupModal() {
  useApp();
  const [healthOut, setHealthOut] = useState<string | null>(null);
  const [solveOut, setSolveOut] = useState<string | null>(null);
  const [peers, setPeers] = useState<DiscoveredPeer[]>([]);
  const [activeRemotePeer, setActiveRemotePeer] = useState<string | null>(null);
  const [scanSummary, setScanSummary] = useState<string | null>(null);
  const [busyAction, setBusyAction] = useState<'health' | 'solve' | 'discover' | 'connect' | null>(null);

  if (!ui.mcpSetupOpen) return null;
  const origin =
    ui.computeMode === 'remote' && ui.computeServerUrl.trim()
      ? ui.computeServerUrl.trim().replace(/\/+$/, '')
      : typeof window !== 'undefined'
        ? window.location.origin
        : 'http://192.168.0.180:5173';

  const scanSubnetForPeers = async () => {
    setBusyAction('discover');
    setScanSummary(null);
    const t0 = performance.now();
    try {
      const resp = await fetch('/api/peers/discover', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ timeout_ms: 150 }),
      });
      const data = await resp.json();
      const dt = Math.round(performance.now() - t0);
      const found: DiscoveredPeer[] = Array.isArray(data?.peers) ? data.peers : [];
      setPeers(found);
      if (data?.active_remote_compute_url) {
        setActiveRemotePeer(data.active_remote_compute_url);
      }
      setScanSummary(
        `Scanned local subnet + loopback in ${dt} ms — found ${found.length} Fieldsmith compute server${found.length === 1 ? '' : 's'}.`,
      );
    } catch (e) {
      setScanSummary(`Subnet scan failed: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBusyAction(null);
    }
  };

  const connectToPeer = async (targetUrl: string) => {
    setBusyAction('connect');
    try {
      const resp = await fetch('/api/peers/connect', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ url: targetUrl, sync_workspace: true }),
      });
      const data = await resp.json();
      if (data?.ok === false) {
        ui.notify(`Failed to connect peer: ${data.error ?? 'unreachable'}`, 'error');
        setHealthOut(JSON.stringify(data, null, 2));
        return;
      }
      const connectedUrl = data?.remote_compute_url ?? (targetUrl !== 'auto' ? targetUrl : null);
      setActiveRemotePeer(connectedUrl);
      if (connectedUrl && connectedUrl !== 'auto') {
        ui.set('computeServerUrl', connectedUrl);
        ui.set('computeMode', 'remote');
      }
      setHealthOut(JSON.stringify(data, null, 2));
      const peerThreads = data?.peer?.threads ?? data?.peer_health?.threads ?? '?';
      ui.notify(
        connectedUrl
          ? `Connected to compute peer ${connectedUrl} (${peerThreads} threads)`
          : 'Disconnected remote compute peer (using local solver)',
        'info',
      );
    } catch (e) {
      ui.notify(`Connect error: ${e instanceof Error ? e.message : String(e)}`, 'error');
    } finally {
      setBusyAction(null);
    }
  };

  const disconnectPeer = async () => {
    setBusyAction('connect');
    try {
      await fetch('/api/peers/connect', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ url: '' }),
      });
      setActiveRemotePeer(null);
      ui.set('computeMode', 'native');
      ui.notify('Disconnected remote compute peer; switched back to local host.', 'info');
    } catch {
      setActiveRemotePeer(null);
      ui.set('computeMode', 'wasm');
    } finally {
      setBusyAction(null);
    }
  };

  const testServerHealth = async () => {
    setBusyAction('health');
    const t0 = performance.now();
    try {
      const resp = await fetch(apiUrl('/api/health'));
      const data = await resp.json();
      const dt = Math.round(performance.now() - t0);
      if (data?.remote_compute_url) {
        setActiveRemotePeer(data.remote_compute_url);
      }
      setHealthOut(JSON.stringify({ latency_ms: dt, ...data }, null, 2));
    } catch (e) {
      setHealthOut(`Connection error: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBusyAction(null);
    }
  };

  const runRemoteNativeSolve = async () => {
    setBusyAction('solve');
    const t0 = performance.now();
    try {
      const fsmText =
        store.doc.root.length > 0 ? printFsm(store.doc) : TUTORIAL_SHEETS[0].starterFsm;
      await fetch(apiUrl('/api/workspace'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          actor: 'mobile-client',
          last_tool: 'remote-solve-sync',
          summary: 'Pushed active .fsm for native multi-core solve',
          doc_fsm: fsmText,
        }),
      });
      const resp = await fetch(apiUrl('/api/mcp'), {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({
          tool: 'solve_study',
          arguments: { include_fine: false },
        }),
      });
      const data = await resp.json();
      const dt = Math.round(performance.now() - t0);
      const textPayload = data?.rpc?.result?.content?.[0]?.text;
      let parsed: unknown = data?.rpc ?? data;
      if (typeof textPayload === 'string') {
        try {
          parsed = JSON.parse(textPayload);
        } catch {
          parsed = textPayload;
        }
      }
      setSolveOut(JSON.stringify({ roundtrip_ms: dt, endpoint: origin, result: parsed }, null, 2));
      ui.notify(`Native server solve completed in ${dt} ms`, 'info');
    } catch (e) {
      setSolveOut(`Remote solve error: ${e instanceof Error ? e.message : String(e)}`);
    } finally {
      setBusyAction(null);
    }
  };

  return (
    <div
      class="sys-scrim"
      data-testid="mcp-setup-modal"
      onClick={(e) => e.target === e.currentTarget && ui.set('mcpSetupOpen', false)}
    >
      <div
        class="sys-modal"
        style={{
          width: 'min(840px, calc(100vw - 24px))',
          height: 'min(760px, calc(100vh - 24px))',
          padding: '0',
        }}
      >
        <div class="sys-header">
          <div class="sys-header-left">
            <span class="sys-badge">COMPUTE, MCP &amp; REST API</span>
            <strong>Mobile / External Compute Backend &amp; AI Co-Pilot Bridge</strong>
          </div>
          <div class="sys-header-right">
            <button
              type="button"
              class="btn small"
              data-testid="close-mcp-setup"
              onClick={() => ui.set('mcpSetupOpen', false)}
            >
              Close
            </button>
          </div>
        </div>
        <div
          style={{
            padding: '14px 16px',
            overflowY: 'auto',
            display: 'flex',
            flexDirection: 'column',
            gap: '12px',
            fontSize: '12px',
            lineHeight: 1.5,
          }}
        >
          <div class="tut-step-card" data-testid="compute-backend-card">
            <strong>0. Mobile &amp; External Compute Backend (F-Droid App / Remote Workstation Offload)</strong>
            <p class="tut-step-text">
              Switch between on-device WebAssembly, local native multi-core execution (<code>fieldsmith-mcp --serve</code> /
              Android <code>arm64-v8a</code> JNI host), or an external compute server over LAN / Tailscale / WireGuard:
            </p>
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: '6px', margin: '8px 0' }}>
              {(
                [
                  { id: 'wasm', label: 'In-Browser WASM (Offline)' },
                  { id: 'native', label: 'Local Native Host (Android / Workstation)' },
                  { id: 'remote', label: 'Remote Compute Server (LAN / Tailscale)' },
                ] as const
              ).map((m) => (
                <button
                  key={m.id}
                  type="button"
                  class={`btn small ${ui.computeMode === m.id ? 'primary' : ''}`}
                  data-testid={`compute-mode-${m.id}`}
                  onClick={() => ui.set('computeMode', m.id)}
                >
                  {m.label}
                </button>
              ))}
            </div>
            {ui.computeMode === 'remote' && (
              <div style={{ display: 'flex', gap: '6px', alignItems: 'center', margin: '6px 0' }}>
                <span style={{ fontFamily: 'var(--mono)', fontSize: '11px', color: 'var(--text-2)' }}>Server URL:</span>
                <input
                  type="text"
                  data-testid="compute-server-url-input"
                  placeholder="http://192.168.0.180:8080"
                  value={ui.computeServerUrl}
                  onInput={(e) => ui.set('computeServerUrl', (e.target as HTMLInputElement).value)}
                  style={{
                    flex: 1,
                    padding: '4px 8px',
                    borderRadius: '6px',
                    border: '1px solid var(--line-strong)',
                    background: 'var(--bg)',
                    color: 'var(--text)',
                    fontFamily: 'var(--mono)',
                    fontSize: '11.5px',
                  }}
                />
              </div>
            )}
            <div style={{ display: 'flex', flexWrap: 'wrap', gap: '6px', marginTop: '8px' }}>
              <button
                type="button"
                class="btn small"
                data-testid="compute-scan-peers-btn"
                disabled={busyAction !== null}
                onClick={scanSubnetForPeers}
              >
                {busyAction === 'discover' ? 'Scanning Subnet…' : '🔍 Scan Subnet for Fieldsmith Servers'}
              </button>
              <button
                type="button"
                class="btn small"
                data-testid="compute-autoconnect-btn"
                disabled={busyAction !== null}
                onClick={() => connectToPeer('auto')}
                title="Automatically discover and link the highest-thread-count Fieldsmith workstation on the LAN"
              >
                {busyAction === 'connect' ? 'Connecting…' : '⚡ Auto-Connect Best Workstation'}
              </button>
              {activeRemotePeer && (
                <button
                  type="button"
                  class="btn small"
                  data-testid="compute-disconnect-btn"
                  disabled={busyAction !== null}
                  onClick={disconnectPeer}
                >
                  Disconnect ({activeRemotePeer})
                </button>
              )}
              <button
                type="button"
                class="btn small"
                data-testid="compute-test-health-btn"
                disabled={busyAction !== null}
                onClick={testServerHealth}
              >
                {busyAction === 'health' ? 'Checking…' : `Test Connection (${origin}/api/health)`}
              </button>
              <button
                type="button"
                class="btn small primary"
                data-testid="compute-run-remote-solve-btn"
                disabled={busyAction !== null}
                onClick={runRemoteNativeSolve}
              >
                {busyAction === 'solve' ? 'Solving on Native Host…' : '⚡ Run Native Multi-Core Solve (/api/mcp)'}
              </button>
            </div>
            {scanSummary && (
              <div
                data-testid="compute-scan-summary"
                style={{
                  marginTop: '8px',
                  fontSize: '11px',
                  fontFamily: 'var(--mono)',
                  color: 'var(--text-2)',
                }}
              >
                {scanSummary}
              </div>
            )}
            {peers.length > 0 && (
              <div
                data-testid="compute-peer-list"
                style={{
                  display: 'flex',
                  flexDirection: 'column',
                  gap: '6px',
                  marginTop: '8px',
                }}
              >
                {peers.map((p) => {
                  const isConnected =
                    activeRemotePeer === p.url ||
                    (ui.computeMode === 'remote' && ui.computeServerUrl.replace(/\/+$/, '') === p.url);
                  return (
                    <div
                      key={p.url}
                      data-testid={`compute-peer-card-${p.hostname}`}
                      style={{
                        display: 'flex',
                        flexWrap: 'wrap',
                        alignItems: 'center',
                        justifyContent: 'space-between',
                        gap: '8px',
                        padding: '8px 10px',
                        borderRadius: '6px',
                        border: isConnected ? '1px solid var(--accent)' : '1px solid var(--line-strong)',
                        background: isConnected ? 'rgba(56, 189, 248, 0.08)' : 'var(--bg)',
                      }}
                    >
                      <div style={{ display: 'flex', flexDirection: 'column', gap: '3px', minWidth: 0 }}>
                        <div style={{ display: 'flex', flexWrap: 'wrap', alignItems: 'center', gap: '6px' }}>
                          <strong style={{ fontFamily: 'var(--mono)', fontSize: '12px' }}>{p.hostname}</strong>
                          <code style={{ fontSize: '11px', color: 'var(--text-2)', wordBreak: 'break-all' }}>{p.url}</code>
                          <span class="sys-badge">
                            {p.threads} threads · {p.os}/{p.arch}
                          </span>
                          <span style={{ fontSize: '11px', color: 'var(--text-3)', fontFamily: 'var(--mono)' }}>
                            {p.latency_ms} ms ({p.via ?? p.source ?? 'lan'})
                          </span>
                        </div>
                        <div style={{ fontSize: '11px', color: 'var(--text-2)' }}>
                          Recommended capacity: up to{' '}
                          <strong>{(p.max_recommended_dofs / 1000).toFixed(0)}k DOFs</strong> · v{p.version}
                        </div>
                      </div>
                      <button
                        type="button"
                        class={`btn small ${isConnected ? '' : 'primary'}`}
                        data-testid={`compute-connect-peer-btn-${p.hostname}`}
                        disabled={busyAction !== null}
                        onClick={() => connectToPeer(p.url)}
                      >
                        {isConnected ? '✓ Connected' : 'Connect & Offload'}
                      </button>
                    </div>
                  );
                })}
              </div>
            )}
            {healthOut && (
              <pre class="tut-mcp-out num" data-testid="compute-health-out" style={{ marginTop: '8px' }}>
                {healthOut}
              </pre>
            )}
            {solveOut && (
              <pre class="tut-mcp-out num" data-testid="compute-solve-out" style={{ marginTop: '8px' }}>
                {solveOut}
              </pre>
            )}
          </div>
          <div class="tut-step-card">
            <strong>1. HTTP REST &amp; MCP Bridge (Zero-Install — Active Now on {origin})</strong>
            <p class="tut-step-text">
              The Fieldsmith web server exposes a live two-way workspace bridge at <code>{origin}/api/mcp</code> and{' '}
              <code>{origin}/api/workspace</code>. Any tool call sent to <code>POST /api/mcp</code> executes against the
              shared workspace and automatically updates this live 3D browser viewport within 1 second:
            </p>
            <pre class="tut-code num" style={{ maxHeight: '220px' }}>{`# 1) Inspect current shared workspace (.fsm model + revision)
curl -s ${origin}/api/workspace | jq .

# 2) Load or update a parameter (e.g. T9 Swirl Atomiser low-NOx orifice_r = 2.1 mm & swirl_offset = 7.2 mm)
curl -s -X POST ${origin}/api/mcp \\
  -H "Content-Type: application/json" \\
  -d '{"tool":"set_param","arguments":{"addr":"orifice_r","value":2.1}}'

curl -s -X POST ${origin}/api/mcp \\
  -H "Content-Type: application/json" \\
  -d '{"tool":"set_param","arguments":{"addr":"swirl_offset","value":7.2}}'

# 3) Run the 3D thermo-mechanical FEA solver and return peak stress, deflection & temperature
curl -s -X POST ${origin}/api/mcp \\
  -H "Content-Type: application/json" \\
  -d '{"tool":"solve_study","arguments":{"include_fine":false}}'`}</pre>
          </div>

          <div class="tut-step-card">
            <strong>2. Stdio MCP Server Setup (Claude Code / Antigravity / Cursor / mcp_config.json)</strong>
            <p class="tut-step-text">
              To register <code>fieldsmith-mcp</code> as a native stdio Model Context Protocol server sharing the same
              live browser workspace file, add this entry to your <code>mcp_config.json</code>:
            </p>
            <pre class="tut-code num" style={{ maxHeight: '200px' }}>{`{
  "mcpServers": {
    "fieldsmith": {
      "command": "/home/ian/Code/fieldsmith/target/release/fieldsmith-mcp",
      "args": [],
      "env": {
        "FIELDSMITH_WORKSPACE": "/home/ian/Code/fieldsmith/web/.fieldsmith-workspace.json"
      }
    }
  }
}`}</pre>
          </div>

          <div class="tut-step-card">
            <strong>3. Available Fieldsmith MCP Tools (24 Tools)</strong>
            <pre class="tut-code num" style={{ maxHeight: '220px' }}>{`• new_document(name, material)            — Reset workspace to a clean .fsm document
• load_fsm(fsm)                           — Load a complete parametric .fsm script (with let variables, CSG & BCs)
• export_fsm()                            — Return canonical .fsm text of the active workspace
• upsert_shape(id, kind, ...)             — Add/update box, cylinder, sphere, capsule, torus, extrude, or revolve
• solve_sketch(preset?, entities?, constraints?, macros?) — Solve 2D parametric sketch (LM + QR DOF & conflict check)
• apply_sketch_feature(id, kind, plane, depth, ...)       — Compile solved 2D sketch onto any 3D face (Boss/Pocket/Revolve)
• insert_fixing(kind, size, ...)          — Auto-tap host & insert screw/bolt/nut/washer/snap-clip + [T_min, T_max]
• set_param(addr, value)                  — Update a let variable (e.g. "orifice_r") or attribute ("nozzle.radius")
• set_material(material, node?)           — Set document material or per-volume material override (e.g. cucrzr on nozzle_body)
• add_region(id, selectors)               — Define a named surface region (e.g. ["chamber_bore.side"])
• add_support(id, region, fixed)          — Fix [x, y, z] displacements on a region
• add_load(id, region, kind/preload, ...) — Apply force (N), pressure (MPa), or bolt preload (N) on a region
• add_temperature(id, region, celsius)    — Apply fixed temperature (°C) boundary condition
• add_convection(id, region, h, t_inf)    — Apply heat-exchanger convective cooling h·(T − T_inf)
• add_heatflux(id, region, q)             — Apply surface heat flux (W/mm²)
• set_fill(family, morph?, cell, cell_min?, erode?, ...) — Configure level-set boundary evolution & multi-zone TPMS infill
• solve_study(include_fine)               — Run 3D P2 tetrahedral FEA + thermal conduction + modal solver
• solve_drone_antenna(preset?, spec?, ...)— Evaluate dual-band VNA S₁₁/S₂₁, 50 Ω Smith Chart & 3D radiation lobes
• optimise_drone_antenna(preset?, ...)    — Auto-optimise CFRP standoff, dielectric dipole lengths, skew & L/C match
• list_coupled_systems()                  — List registered multi-physics PDE + Adjoint packs
• optimise_coupled_system(system_id, ...) — Step coupled PDE design levers along exact Adjoint gradients
• discover_compute_peers(timeout_ms?)     — Scan LAN / subnet (UDP :18790 + /api/health sweep) for Fieldsmith workstations
• connect_compute_peer(url, sync_workspace?) — Link remote workstation ("auto" or URL) to offload solve jobs
• disconnect_compute_peer()               — Disconnect remote compute peer and resume local execution`}</pre>
          </div>
        </div>
      </div>
    </div>
  );
}

function TutorialsOverlayHost() {
  useApp();
  return (
    <>
      <TutorialsDrawer />
      <McpSetupModal />
    </>
  );
}
{
  const idx = viewOverlays.findIndex((fn) => fn.name === 'TutorialsOverlayHost');
  if (idx >= 0) viewOverlays[idx] = TutorialsOverlayHost;
  else viewOverlays.push(TutorialsOverlayHost);
}

function TutorialsStatusItem() {
  return (
    <>
      <span class="sep">|</span>
      <button
        class="btn ghost small"
        data-testid="footer-analytic-tests"
        style={{ height: '20px', padding: '0 6px', fontSize: '11px', color: '#22d3ee' }}
        title="Open Closed-Form Analytic Verification Benchmarks (A1–A14)"
        onClick={() => {
          ui.set('tutorialMinimised', false);
          ui.set('tutorialCategoryFilter', 'analytic');
          ui.set('tutorialContentsOpen', true);
          ui.set('tutorialsOpen', true);
        }}
      >
        📐 Analytic Tests (A1–A14)
      </button>
      <span class="sep">|</span>
      <button
        class="btn ghost small"
        data-testid="footer-tutorials"
        style={{ height: '20px', padding: '0 6px', fontSize: '11px' }}
        onClick={toggleTutorialsDrawer}
      >
        📘 Tutorials &amp; Case Studies (T1–T17)
      </button>
      <span class="sep">|</span>
      <button
        class="btn ghost small"
        data-testid="footer-mcp-setup"
        style={{ height: '20px', padding: '0 6px', fontSize: '11px', color: 'var(--accent)' }}
        title="View MCP Server & REST API Co-Pilot Setup Instructions"
        onClick={() => ui.set('mcpSetupOpen', !ui.mcpSetupOpen)}
      >
        MCP / API Setup
      </button>
    </>
  );
}
{
  const idx = statusItems.findIndex((fn) => fn.name === 'TutorialsStatusItem');
  if (idx >= 0) statusItems[idx] = TutorialsStatusItem;
  else statusItems.push(TutorialsStatusItem);
}

extraCommands.push(() => [
  {
    id: 'tutorials.open',
    group: 'Help & Theory',
    label: 'Open Interactive Tutorials, Analytic Benchmarks & Case Studies (T1–T17 · A1–A14)',
    run: () => toggleTutorialsDrawer(),
  },
  {
    id: 'mcp.setup',
    group: 'Help & Theory',
    label: 'Open Fieldsmith MCP & REST API Setup Guide',
    run: () => ui.set('mcpSetupOpen', true),
  },
]);
