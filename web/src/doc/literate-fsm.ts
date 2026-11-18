// Literate `.fsm` single-file specification & parser (Stéphane Popinet Basilisk / Gerris style):
// Every example, analytic verification test (A1–A14), guided tutorial (T1–T7, T14–T15), and
// multiphysics case study (T8–T13, T16–T17) lives in its own standalone `.fsm` file under
// `examples/{analytic,tutorials,case-studies}/`.
//
// Because `fieldsmith_doc::fsm::parse` natively treats `#` lines as comments, a literate `.fsm`
// file is simultaneously:
//   1. Directly executable by `fieldsmith solve <file.fsm>`, `fieldsmith_doc::fsm::parse`,
//      WASM `parseFsm()`, and MCP `load_fsm`.
//   2. Self-defining for the in-app Tutorials / Analytic Verification drawer and the
//      repository `docs/` Markdown generator via `# @...` directives inside
//      `# === FIELDSMITH LITERATE CASE ===` ... `# === END LITERATE HEADER ===`.

import type { Lens } from './store';

export interface TutorialStep {
  title: string;
  lens?: Lens | 'systems';
  instruction: string;
  expected: string;
}

export interface TheorySection {
  heading: string;
  equations: string;
  body: string;
}

export interface McpFlowStep {
  title: string;
  description: string;
  tool: string;
  arguments: Record<string, unknown>;
}

export type TutorialCategory = 'tutorial' | 'analytic' | 'case-study';

export type TutorialLevel =
  | 'Beginner'
  | 'Intermediate'
  | 'Advanced'
  | 'Flagship Multi-Physics'
  | 'Analytic Verification';

export interface AnalyticCheckRow {
  quantity: string;
  formula: string;
  analyticRef: string;
  measuredFea: string;
  relError: string;
  tolerance: string;
}

export interface LiterateAssertRow {
  label: string;
  metric: string;
  target: string;
  tolerance: string;
  unit: string;
}

export interface EvaluatedLiterateAssert {
  assert: LiterateAssertRow;
  liveValue: number | null;
  errorOrMargin: string;
  passed: boolean;
}

/** Evaluate a `LiterateAssertRow` against a solved `LevelResult` in the browser. */
export function evaluateLiterateAssertOnLevel(
  asrt: LiterateAssertRow,
  lvl: {
    meta: {
      cases: {
        name: string;
        max_displacement: number;
        max_vm_elem: number;
        max_von_mises: number;
        compliance: number;
        load_reports?: { resultant: [number, number, number] }[];
        contact_reports?: {
          id: string;
          region: string;
          max_contact_pressure_mpa: number;
          contact_force_n: number;
          total_area_mm2: number;
          active_area_mm2: number;
          contact_area_ratio: number;
          max_penetration_mm: number;
          max_liftoff_mm: number;
          active_set_iters: number;
        }[];
      }[];
      max_layer_peel?: number[];
      yield_utilization?: (number | null)[];
      buckling_utilization?: (number | null)[];
      mass_g?: number;
      min_peak_peel?: number;
      max_peak_peel?: number;
      solid_cases?: { name: string; max_vm_elem: number; max_displacement: number }[];
      fill?: {
        mass_g: number;
        solid_mass_g: number;
        hx?: {
          effective_permeability_mm2: number;
          mean_area_density_per_m: number;
          wetted_area_cm2: number;
          mean_hydraulic_diam_mm: number;
          pressure_drop_kpa: number;
          cold_pressure_drop_kpa: number;
          darcy_share_pct: number;
          hot_pumping_power_w: number;
          pumping_power_w: number;
          mean_reynolds: number;
          ua_w_k: number;
          ntu: number;
          effectiveness: number;
          heat_duty_w: number;
          power_density_kw_per_l: number;
          hot_outlet_temp_c: number;
          cold_outlet_temp_c: number;
        } | null;
      };
      modes?: { frequencies: number[] };
      system?: {
        sections?: {
          name: string;
          min: number;
          max: number;
          mean: number;
          entry: number;
          exit: number;
        }[];
        objectives?: {
          name: string;
          value: number;
        }[];
      };
      transient?: {
        case_name: string;
        peak_vm_elem: number;
        peak_von_mises: number;
        peak_displacement: number;
        t_at_peak_vm_s: number;
        steady_limit_vm_elem?: number | null;
        shock_amplification?: number | null;
        final_min_temp_c: number;
        final_max_temp_c: number;
        final_vm_elem?: number;
        timeline?: { max_vm_elem: number }[];
      }[];
    };
    temperature?: (Float64Array | null)[];
  },
): EvaluatedLiterateAssert {
  const m = asrt.metric.trim();
  let val: number | null = null;

  if (m.startsWith('mode_')) {
    const rest = m.slice('mode_'.length);
    if (rest === 'split_12_pct') {
      const freqs = lvl.meta.modes?.frequencies ?? [];
      if (freqs.length >= 2) {
        const mean = Math.max(1e-12, 0.5 * (freqs[0] + freqs[1]));
        val = (100 * Math.abs(freqs[1] - freqs[0])) / mean;
      }
    } else {
      const idx1 = Number.parseInt(rest, 10);
      const freqs = lvl.meta.modes?.frequencies ?? [];
      if (Number.isFinite(idx1) && idx1 >= 1 && idx1 <= freqs.length) {
        val = freqs[idx1 - 1];
      }
    }
  } else if (m === 'mass_g') {
    val = lvl.meta.fill?.mass_g ?? lvl.meta.mass_g ?? null;
  } else if (m === 'solid_mass_g') {
    val = lvl.meta.fill?.solid_mass_g ?? lvl.meta.mass_g ?? null;
  } else if (m === 'graded_mass_g') {
    val = lvl.meta.fill?.mass_g ?? null;
  } else if (m === 'mass_saved_pct') {
    const f = lvl.meta.fill;
    if (f && f.solid_mass_g > 0) {
      val = 100 * (1 - f.mass_g / f.solid_mass_g);
    }
  } else if (m === 'min_peak_peel') {
    val = lvl.meta.min_peak_peel ?? null;
  } else if (m === 'max_peak_peel') {
    val = lvl.meta.max_peak_peel ?? null;
  } else if (m.startsWith('sys.') || m.startsWith('sys_')) {
    const objName = m.startsWith('sys.') ? m.slice('sys.'.length) : m.slice('sys_'.length);
    const obj = (lvl.meta.system?.objectives ?? []).find((o) => o.name === objName);
    val = obj ? obj.value : null;
  } else if (m.startsWith('section.')) {
    const restSec = m.slice('section.'.length);
    const dot = restSec.indexOf('.');
    const secName = dot >= 0 ? restSec.slice(0, dot) : restSec;
    const red = dot >= 0 ? restSec.slice(dot + 1) : 'mean';
    const sec = (lvl.meta.system?.sections ?? []).find((s) => s.name === secName);
    if (sec) {
      if (red === 'mean' || red === 'avg') val = sec.mean;
      else if (red === 'min') val = sec.min;
      else if (red === 'max') val = sec.max;
      else if (red === 'entry' || red === 'inlet') val = sec.entry;
      else if (red === 'exit' || red === 'outlet') val = sec.exit;
    }
  } else {
    const hxKey = m.startsWith('graded.') ? m.slice('graded.'.length) : m;
    const hx = lvl.meta.fill?.hx ?? null;
    if (
      hx &&
      [
        'hx_pressure_drop_kpa',
        'pressure_drop_kpa',
        'hx_cold_pressure_drop_kpa',
        'hx_darcy_share_pct',
        'hx_hot_pumping_power_w',
        'hx_pumping_power_w',
        'pumping_power_w',
        'hx_permeability_mm2',
        'hx_area_density_per_m',
        'hx_wetted_area_cm2',
        'hx_hydraulic_diam_mm',
        'hx_reynolds',
        'hx_ua_w_k',
        'hx_ntu',
        'hx_effectiveness',
        'hx_heat_duty_w',
        'hx_power_density_kw_l',
        'hot_outlet_temp_c',
        'hx_hot_outlet_temp_c',
        'cold_outlet_temp_c',
        'hx_cold_outlet_temp_c',
      ].includes(hxKey)
    ) {
      if (hxKey === 'hx_pressure_drop_kpa' || hxKey === 'pressure_drop_kpa') val = hx.pressure_drop_kpa;
      else if (hxKey === 'hx_cold_pressure_drop_kpa') val = hx.cold_pressure_drop_kpa;
      else if (hxKey === 'hx_darcy_share_pct') val = hx.darcy_share_pct;
      else if (hxKey === 'hx_hot_pumping_power_w') val = hx.hot_pumping_power_w;
      else if (hxKey === 'hx_pumping_power_w' || hxKey === 'pumping_power_w') val = hx.pumping_power_w;
      else if (hxKey === 'hx_permeability_mm2') val = hx.effective_permeability_mm2;
      else if (hxKey === 'hx_area_density_per_m') val = hx.mean_area_density_per_m;
      else if (hxKey === 'hx_wetted_area_cm2') val = hx.wetted_area_cm2;
      else if (hxKey === 'hx_hydraulic_diam_mm') val = hx.mean_hydraulic_diam_mm;
      else if (hxKey === 'hx_reynolds') val = hx.mean_reynolds;
      else if (hxKey === 'hx_ua_w_k') val = hx.ua_w_k;
      else if (hxKey === 'hx_ntu') val = hx.ntu;
      else if (hxKey === 'hx_effectiveness') val = hx.effectiveness;
      else if (hxKey === 'hx_heat_duty_w') val = hx.heat_duty_w;
      else if (hxKey === 'hx_power_density_kw_l') val = hx.power_density_kw_per_l;
      else if (hxKey === 'hot_outlet_temp_c' || hxKey === 'hx_hot_outlet_temp_c') val = hx.hot_outlet_temp_c;
      else if (hxKey === 'cold_outlet_temp_c' || hxKey === 'hx_cold_outlet_temp_c') val = hx.cold_outlet_temp_c;
    } else {
    let useSolidRef = false;
    let rest = m;
    if (rest.startsWith('graded.')) {
      rest = rest.slice('graded.'.length);
    } else if (rest.startsWith('solid.')) {
      useSolidRef = true;
      rest = rest.slice('solid.'.length);
    }
    const colonIdx = rest.indexOf(':');
    const field = colonIdx >= 0 ? rest.slice(0, colonIdx).trim() : rest.trim();
    const caseSel = colonIdx >= 0 ? rest.slice(colonIdx + 1).trim() : '';

    const findCaseIdx = (names: { name: string }[]): number => {
      if (!caseSel) return names.length > 0 ? 0 : -1;
      const idx1 = Number.parseInt(caseSel, 10);
      if (String(idx1) === caseSel && idx1 >= 1 && idx1 <= names.length) {
        return idx1 - 1;
      }
      const lower = caseSel.toLowerCase();
      return names.findIndex((c) => c.name.toLowerCase() === lower || c.name.includes(caseSel));
    };

    if (useSolidRef && lvl.meta.solid_cases && lvl.meta.solid_cases.length > 0) {
      const ci = findCaseIdx(lvl.meta.solid_cases);
      if (ci >= 0) {
        const sc = lvl.meta.solid_cases[ci];
        if (field === 'max_displacement' || field === 'deflection') val = sc.max_displacement;
        else if (field === 'max_vm_elem' || field === 'vm') val = sc.max_vm_elem;
      }
    } else if (
      ['yield_utilization', 'yield_util', 'buckling_utilization', 'buckling_util'].includes(field)
    ) {
      const ci = findCaseIdx(lvl.meta.cases);
      if (ci >= 0) {
        if (field === 'yield_utilization' || field === 'yield_util') {
          val = lvl.meta.yield_utilization?.[ci] ?? null;
        } else {
          val = lvl.meta.buckling_utilization?.[ci] ?? null;
        }
      }
    } else if (
      [
        'peak_transient_vm',
        'peak_transient_vm_elem',
        'peak_transient_von_mises',
        'peak_transient_disp',
        'peak_transient_displacement',
        't_at_peak_vm',
        't_at_peak_vm_s',
        'final_max_temp',
        'final_min_temp',
        'final_vm_elem',
        'steady_limit_vm_elem',
        'shock_amplification',
      ].includes(field)
    ) {
      const trs = lvl.meta.transient ?? [];
      const ti = findCaseIdx(trs.map((r) => ({ name: r.case_name })));
      if (ti >= 0) {
        const tr = trs[ti];
        if (field === 'peak_transient_vm' || field === 'peak_transient_vm_elem') val = tr.peak_vm_elem;
        else if (field === 'peak_transient_von_mises') val = tr.peak_von_mises;
        else if (field === 'peak_transient_disp' || field === 'peak_transient_displacement') val = tr.peak_displacement;
        else if (field === 't_at_peak_vm' || field === 't_at_peak_vm_s') val = tr.t_at_peak_vm_s;
        else if (field === 'final_max_temp') val = tr.final_max_temp_c;
        else if (field === 'final_min_temp') val = tr.final_min_temp_c;
        else if (field === 'final_vm_elem') {
          const tl = tr.timeline ?? [];
          val = tr.final_vm_elem ?? (tl.length > 0 ? tl[tl.length - 1].max_vm_elem : null);
        } else if (field === 'steady_limit_vm_elem') val = tr.steady_limit_vm_elem ?? null;
        else if (field === 'shock_amplification') val = tr.shock_amplification ?? null;
      }
    } else if (
      [
        'max_contact_pressure',
        'contact_pressure',
        'contact_force_n',
        'contact_force',
        'contact_area_ratio',
        'active_contact_area_mm2',
        'max_penetration_mm',
        'max_liftoff_mm',
        'liftoff_mm',
      ].includes(field)
    ) {
      const slashIdx = caseSel.indexOf('/');
      const casePart = slashIdx >= 0 ? caseSel.slice(0, slashIdx).trim() : caseSel;
      const contactIdOpt = slashIdx >= 0 ? caseSel.slice(slashIdx + 1).trim() : '';
      let chosenCase = lvl.meta.cases[0];
      let contactFilter = contactIdOpt;
      if (!casePart) {
        chosenCase =
          lvl.meta.cases.find((c) => (c.contact_reports?.length ?? 0) > 0) ?? lvl.meta.cases[0];
      } else {
        const idx1 = Number.parseInt(casePart, 10);
        if (String(idx1) === casePart && idx1 >= 1 && idx1 <= lvl.meta.cases.length) {
          chosenCase = lvl.meta.cases[idx1 - 1];
        } else {
          const lower = casePart.toLowerCase();
          const byName = lvl.meta.cases.find(
            (c) => c.name.toLowerCase() === lower || c.name.includes(casePart),
          );
          if (byName) {
            chosenCase = byName;
          } else if (!contactIdOpt) {
            const byContact = lvl.meta.cases.find((c) =>
              (c.contact_reports ?? []).some((r) => r.id.toLowerCase() === lower),
            );
            if (byContact) {
              chosenCase = byContact;
              contactFilter = casePart;
            }
          }
        }
      }
      const reps = (chosenCase?.contact_reports ?? []).filter(
        (r) => !contactFilter || r.id.toLowerCase() === contactFilter.toLowerCase(),
      );
      if (reps.length > 0) {
        if (field === 'max_contact_pressure' || field === 'contact_pressure') {
          val = Math.max(...reps.map((r) => r.max_contact_pressure_mpa));
        } else if (field === 'contact_force_n' || field === 'contact_force') {
          val = reps.reduce((s, r) => s + r.contact_force_n, 0);
        } else if (field === 'active_contact_area_mm2') {
          val = reps.reduce((s, r) => s + r.active_area_mm2, 0);
        } else if (field === 'contact_area_ratio') {
          const act = reps.reduce((s, r) => s + r.active_area_mm2, 0);
          const tot = reps.reduce((s, r) => s + r.total_area_mm2, 0);
          val = tot > 0 ? act / tot : 0;
        } else if (field === 'max_penetration_mm') {
          val = Math.max(...reps.map((r) => r.max_penetration_mm));
        } else if (field === 'max_liftoff_mm' || field === 'liftoff_mm') {
          val = Math.max(...reps.map((r) => r.max_liftoff_mm));
        }
      }
    } else {
      const ci = findCaseIdx(lvl.meta.cases);
      if (ci >= 0) {
        const c = lvl.meta.cases[ci];
        if (field === 'max_displacement' || field === 'deflection') val = c.max_displacement;
        else if (field === 'max_vm_elem' || field === 'vm') val = c.max_vm_elem;
        else if (field === 'max_von_mises') val = c.max_von_mises;
        else if (field === 'max_layer_peel' || field === 'peel') val = lvl.meta.max_layer_peel?.[ci] ?? null;
        else if (field === 'compliance') val = c.compliance;
        else if (field === 'max_temp') {
          const tf = lvl.temperature?.[ci];
          if (tf && tf.length > 0) {
            let hi = -Infinity;
            for (let i = 0; i < tf.length; i++) if (tf[i] > hi) hi = tf[i];
            val = hi;
          }
        } else if (field === 'min_temp') {
          const tf = lvl.temperature?.[ci];
          if (tf && tf.length > 0) {
            let lo = Infinity;
            for (let i = 0; i < tf.length; i++) if (tf[i] < lo) lo = tf[i];
            val = lo;
          }
        } else if (field === 'resultant_mag') {
          const r = c.load_reports?.[0]?.resultant;
          if (r) val = Math.hypot(r[0], r[1], r[2]);
        }
      }
    }
    }
  }

  if (val === null || !Number.isFinite(val)) {
    return { assert: asrt, liveValue: null, errorOrMargin: 'N/A', passed: false };
  }

  const tgt = asrt.target.trim();
  const tolRaw = asrt.tolerance.trim();
  const fmtSigned = (d: number) => `${d >= 0 ? '+' : ''}${d.toFixed(4)}`;

  if (tgt.startsWith('<=') || tgt.startsWith('≤')) {
    const b = Number.parseFloat(tgt.replace(/^(<=|≤)\s*/, ''));
    const ok = val <= b;
    return { assert: asrt, liveValue: val, errorOrMargin: fmtSigned(val - b), passed: ok };
  }
  if (tgt.startsWith('>=') || tgt.startsWith('≥')) {
    const b = Number.parseFloat(tgt.replace(/^(>=|≥)\s*/, ''));
    const ok = val >= b;
    return { assert: asrt, liveValue: val, errorOrMargin: fmtSigned(val - b), passed: ok };
  }
  if (tgt.startsWith('<')) {
    const b = Number.parseFloat(tgt.slice(1).trim());
    const ok = val < b;
    return { assert: asrt, liveValue: val, errorOrMargin: fmtSigned(val - b), passed: ok };
  }
  if (tgt.startsWith('>')) {
    const b = Number.parseFloat(tgt.slice(1).trim());
    const ok = val > b;
    return { assert: asrt, liveValue: val, errorOrMargin: fmtSigned(val - b), passed: ok };
  }

  const expected = Number.parseFloat(tgt);
  if (!Number.isFinite(expected)) {
    return { assert: asrt, liveValue: val, errorOrMargin: 'bad target', passed: false };
  }
  const absErr = Math.abs(val - expected);
  if (tolRaw.endsWith('%')) {
    const tolPct = Number.parseFloat(tolRaw.slice(0, -1).replace(/^(<=|≤)\s*/, '').trim());
    const denom = Math.max(1e-12, Math.abs(expected));
    const relPct = (100 * absErr) / denom;
    return {
      assert: asrt,
      liveValue: val,
      errorOrMargin: `${relPct.toFixed(2)}%`,
      passed: relPct <= tolPct,
    };
  }
  const tolAbs = Number.parseFloat(tolRaw.replace(/^(<=|≤|±)\s*/, '').trim()) || 1e-3;
  return {
    assert: asrt,
    liveValue: val,
    errorOrMargin: `±${absErr.toFixed(4)}`,
    passed: absErr <= tolAbs,
  };
}

export interface TutorialSheet {
  id: string;
  code: string;
  category?: TutorialCategory;
  docPath?: string;
  fsmPath?: string;
  verificationRefs?: string[];
  analyticChecks?: AnalyticCheckRow[];
  asserts?: LiterateAssertRow[];
  level: TutorialLevel;
  title: string;
  subtitle: string;
  verificationBadge: string;
  /** Whether a shipped verification report backs the badge. Absent ⇒ 'verified' (legacy default).
   *  'pending' means the benchmark is defined but its V# report has not been generated; the badge
   *  must not claim PASS and the UI must not render a green ✓. Enforced by web/tests/registry.mjs. */
  verificationStatus?: 'verified' | 'pending';
  starterFsmName: string;
  starterFsm: string;
  opensSystemsWorkbench?: boolean;
  userSteps: TutorialStep[];
  theoryChapter: {
    chapterTitle: string;
    summary: string;
    sections: TheorySection[];
  };
  mcpSteps: McpFlowStep[];
}

export const DEFAULT_DOC_PATHS: Record<string, string> = {
  T1: 'docs/tutorials/T1-cantilever-bracket.md',
  T2: 'docs/tutorials/T2-parametric-cantilever-sweeps.md',
  T3: 'docs/tutorials/T3-thermo-elastic-bracket.md',
  T4: 'docs/tutorials/T4-modal-vibration-frequencies.md',
  T5: 'docs/tutorials/T5-additive-layer-peel-orientation.md',
  T6: 'docs/tutorials/T6-stress-graded-tpms-gyroid.md',
  T7: 'docs/tutorials/T7-human-ai-codesign-mcp.md',
  T8: 'docs/case-studies/T8-float-bath-cvd-coater.md',
  T9: 'docs/case-studies/T9-swirl-atomiser-low-nox-combustor.md',
  T10: 'docs/case-studies/T10-bimetal-preloaded-flange-valve.md',
  T11: 'docs/case-studies/T11-multizone-tpms-heat-exchanger.md',
  T12: 'docs/case-studies/T12-levelset-evolver-aerospace-bracket.md',
  T13: 'docs/case-studies/T13-solar-parabolic-candle-igniter.md',
  T14: 'docs/tutorials/T14-fastener-torque-weibull-preload.md',
  T15: 'docs/tutorials/T15-parametric-2d-sketch-constraints.md',
  T16: 'docs/case-studies/T16-deadcat-fpv-drone-gemini-antenna.md',
  T17: 'docs/case-studies/T17-regenerative-rocket-nozzle.md',
  A1: 'docs/analytic-tests/A1-cantilever-beam-theory.md',
  A2: 'docs/analytic-tests/A2-kirsch-plate-with-hole.md',
  A3: 'docs/analytic-tests/A3-de-laval-revolve-pappus-volume.md',
  A4: 'docs/analytic-tests/A4-lame-pressurized-bicylinder.md',
  A5: 'docs/analytic-tests/A5-regenerative-cooling-robin-lame.md',
  A6: 'docs/analytic-tests/A6-tubular-chamber-modal-doublet.md',
  A7: 'docs/analytic-tests/A7-distributed-face-tractions.md',
  A8: 'docs/analytic-tests/A8-thermoelastic-free-and-clamped-bar.md',
  A9: 'docs/analytic-tests/A9-dual-compliance-bounds-prager-synge.md',
  A10: 'docs/analytic-tests/A10-multimaterial-series-bar-and-preload.md',
  A11: 'docs/analytic-tests/A11-tpms-cubic-anisotropy-and-gibson-ashby.md',
  A12: 'docs/analytic-tests/A12-additive-overhang-and-layer-peel.md',
  A13: 'docs/analytic-tests/A13-fastener-thread-torque-and-weibull.md',
  A14: 'docs/analytic-tests/A14-rf-smith-chart-vswr-dipole-lobes.md',
};

export const DEFAULT_FSM_PATHS: Record<string, string> = {
  T1: 'examples/tutorials/T1-cantilever-bracket.fsm',
  T2: 'examples/tutorials/T2-parametric-cantilever-sweeps.fsm',
  T3: 'examples/tutorials/T3-thermo-elastic-bracket.fsm',
  T4: 'examples/tutorials/T4-modal-vibration-frequencies.fsm',
  T5: 'examples/tutorials/T5-additive-layer-peel-orientation.fsm',
  T6: 'examples/tutorials/T6-stress-graded-tpms-gyroid.fsm',
  T7: 'examples/tutorials/T7-human-ai-codesign-mcp.fsm',
  T8: 'examples/case-studies/T8-float-bath-cvd-coater.fsm',
  T9: 'examples/case-studies/T9-swirl-atomiser-low-nox-combustor.fsm',
  T10: 'examples/case-studies/T10-bimetal-preloaded-flange-valve.fsm',
  T11: 'examples/case-studies/T11-multizone-tpms-heat-exchanger.fsm',
  T12: 'examples/case-studies/T12-levelset-evolver-aerospace-bracket.fsm',
  T13: 'examples/case-studies/T13-solar-parabolic-candle-igniter.fsm',
  T14: 'examples/tutorials/T14-fastener-torque-weibull-preload.fsm',
  T15: 'examples/tutorials/T15-parametric-2d-sketch-constraints.fsm',
  T16: 'examples/case-studies/T16-deadcat-fpv-drone-gemini-antenna.fsm',
  T17: 'examples/case-studies/T17-regenerative-rocket-nozzle.fsm',
  T18: 'examples/case-studies/T18-multi-regime-free-surface-spout-and-peregrine-wave.fsm',
  T19: 'examples/tutorials/T19-sheet-metal-clinching-slock-nonlinear-fem.fsm',
  T20: 'examples/tutorials/T20-declarative-cooled-pipe.fsm',
  T21: 'examples/tutorials/T21-epistemic-robust-cantilever.fsm',
  T22: 'examples/tutorials/T22-preloaded-bolted-flange-joint.fsm',
  T23: 'examples/tutorials/T23-clinching-process-window-and-slock-codesign.fsm',
  T24: 'examples/tutorials/T24-transient-thermal-shock-quench-and-ramp.fsm',
  T25: 'examples/tutorials/T25-preloaded-flange-gasket-liftoff-contact.fsm',
  T26: 'examples/tutorials/T26-graded-gyroid-counterflow-recuperator.fsm',
  T27: 'examples/tutorials/T27-sdf-coupled-regenerative-cooling-jacket.fsm',
  T28: 'examples/tutorials/T28-bench-thermocouple-and-deflection-map-calibration.fsm',
  A1: 'examples/analytic/A1-cantilever-beam-theory.fsm',
  A2: 'examples/analytic/A2-kirsch-plate-with-hole.fsm',
  A3: 'examples/analytic/A3-de-laval-revolve-pappus-volume.fsm',
  A4: 'examples/analytic/A4-lame-pressurized-bicylinder.fsm',
  A5: 'examples/analytic/A5-regenerative-cooling-robin-lame.fsm',
  A6: 'examples/analytic/A6-tubular-chamber-modal-doublet.fsm',
  A7: 'examples/analytic/A7-distributed-face-tractions.fsm',
  A8: 'examples/analytic/A8-thermoelastic-free-and-clamped-bar.fsm',
  A9: 'examples/analytic/A9-dual-compliance-bounds-prager-synge.fsm',
  A10: 'examples/analytic/A10-multimaterial-series-bar-and-preload.fsm',
  A11: 'examples/analytic/A11-tpms-cubic-anisotropy-and-gibson-ashby.fsm',
  A12: 'examples/analytic/A12-additive-overhang-and-layer-peel.fsm',
  A13: 'examples/analytic/A13-fastener-thread-torque-and-weibull.fsm',
  A14: 'examples/analytic/A14-rf-smith-chart-vswr-dipole-lobes.fsm',
  A15: 'examples/analytic/A15-young-laplace-droplet-and-rayleigh-lamb.fsm',
  A16: 'examples/analytic/A16-jurin-capillary-rise-and-tpms-gyroid-wick.fsm',
  A17: 'examples/analytic/A17-gerris-capillary-suite-spurious-capwave-plateau.fsm',
  A18: 'examples/analytic/A18-gerris-navier-stokes-poiseuille-couette-taylor-thacker.fsm',
  A19: 'examples/analytic/A19-sheet-metal-clinching-lee-kim-breda.fsm',
  A20: 'examples/analytic/A20-transient-biot-quench-thermal-shock.fsm',
  A21: 'examples/analytic/A21-signorini-unilateral-contact-and-prying-liftoff.fsm',
  A22: 'examples/analytic/A22-darcy-forchheimer-tpms-porous-hx.fsm',
  A23: 'examples/analytic/A23-sdf-venturi-section-and-coupled-1d-flow.fsm',
  A24: 'examples/analytic/A24-bayesian-map-coupon-and-thermal-calibration.fsm',
  A25: 'examples/analytic/A25-spurious-capillary-eddies-geometry-error.fsm',
};

export const DEFAULT_VERIFICATION_REFS: Record<string, string[]> = {
  T1: ['docs/verification/V1.md', 'docs/verification/V2.md'],
  T2: ['docs/verification/V23.md', 'docs/verification/V34.md', 'docs/verification/V35.md'],
  T3: ['docs/verification/V19.md', 'docs/verification/V20.md'],
  T4: ['docs/verification/V13.md'],
  T5: ['docs/verification/V45.md', 'docs/verification/V46.md'],
  T6: ['docs/verification/V6.md', 'docs/verification/V7.md', 'docs/verification/V48.md'],
  T7: ['docs/verification/V22-MCP.md', 'docs/verification/V51.md'],
  T8: ['docs/verification/V52.md'],
  T9: ['docs/verification/V51.md'],
  T10: ['docs/verification/V12.md', 'docs/verification/V53.md'],
  T11: ['docs/verification/V36.md', 'docs/verification/V54.md'],
  T12: ['docs/verification/V50.md', 'docs/verification/V55.md'],
  T13: ['docs/verification/V56.md'],
  T14: ['docs/verification/V57.md'],
  T15: ['docs/verification/V58.md'],
  T16: ['docs/verification/V59.md'],
  T17: ['docs/verification/V60.md', 'docs/verification/V20.md'],
  T18: ['examples/case-studies/T18-multi-regime-free-surface-spout-and-peregrine-wave.fsm'],
  T19: ['docs/verification/V53.md', 'docs/verification/V60.md'],
  T20: ['docs/verification/V63.md', 'crates/fieldsmith-verify/tests/v63_architecture_lock.rs'],
  T21: ['docs/verification/V63.md', 'crates/fieldsmith-verify/tests/v63_architecture_lock.rs'],
  T22: ['docs/verification/V57.md', 'docs/verification/V63.md', 'crates/fieldsmith-verify/tests/v57_fixings_torque_weibull.rs', 'crates/fieldsmith-verify/tests/v63_architecture_lock.rs'],
  T23: ['docs/verification/V53.md', 'docs/verification/V63.md', 'crates/fieldsmith-verify/tests/v63_architecture_lock.rs'],
  T24: ['docs/verification/V20.md', 'docs/verification/V63.md'],
  T25: ['docs/verification/V63.md', 'examples/analytic/A21-signorini-unilateral-contact-and-prying-liftoff.fsm'],
  T26: ['docs/verification/V36.md', 'docs/verification/V54.md', 'docs/verification/V63.md', 'examples/analytic/A22-darcy-forchheimer-tpms-porous-hx.fsm'],
  T27: ['docs/verification/V20.md', 'docs/verification/V36.md', 'docs/verification/V63.md', 'examples/analytic/A23-sdf-venturi-section-and-coupled-1d-flow.fsm'],
  T28: ['docs/verification/V20.md', 'docs/verification/V63.md', 'crates/fieldsmith-core/src/calibrate.rs'],
  A1: ['docs/verification/V1.md', 'docs/verification/V2.md'],
  A2: ['docs/verification/V3.md'],
  A3: ['docs/verification/V4.md', 'docs/verification/V60.md'],
  A4: ['docs/verification/V20.md', 'docs/verification/V60.md'],
  A5: ['docs/verification/V20.md', 'docs/verification/V60.md'],
  A6: ['docs/verification/V13.md', 'docs/verification/V60.md'],
  A7: ['docs/verification/V15.md'],
  A8: ['docs/verification/V19.md'],
  A9: ['docs/verification/V34.md', 'docs/verification/V35.md'],
  A10: ['docs/verification/V53.md'],
  A11: ['docs/verification/V6.md', 'docs/verification/V36.md', 'docs/verification/V42.md', 'docs/verification/V48.md'],
  A12: ['docs/verification/V45.md', 'docs/verification/V46.md'],
  A13: ['docs/verification/V57.md'],
  A14: ['docs/verification/V59.md'],
  A15: ['examples/analytic/A15-young-laplace-droplet-and-rayleigh-lamb.fsm'],
  A16: ['examples/analytic/A16-jurin-capillary-rise-and-tpms-gyroid-wick.fsm'],
  A17: ['docs/verification/V63.md'],
  A18: ['examples/analytic/A18-gerris-navier-stokes-poiseuille-couette-taylor-thacker.fsm'],
  A19: ['docs/verification/V53.md', 'docs/verification/V60.md'],
  A20: ['docs/verification/V20.md', 'docs/verification/V63.md'],
  A21: ['docs/verification/V63.md', 'crates/fieldsmith-solve/src/contact.rs'],
  A22: ['docs/verification/V36.md', 'docs/verification/V63.md', 'crates/fieldsmith-core/src/porous_hx.rs'],
  A23: ['docs/verification/V4.md', 'docs/verification/V20.md', 'docs/verification/V63.md', 'crates/fieldsmith-core/src/declarative_system.rs'],
  A24: ['docs/verification/V1.md', 'docs/verification/V20.md', 'docs/verification/V63.md', 'crates/fieldsmith-core/src/calibrate.rs'],
};

export function getCategoryForSheet(sheet: Pick<TutorialSheet, 'id' | 'category'>): TutorialCategory {
  if (sheet.category) return sheet.category;
  if (sheet.id.startsWith('A')) return 'analytic';
  if (['T8', 'T9', 'T10', 'T11', 'T12', 'T13', 'T16', 'T17', 'T18'].includes(sheet.id)) {
    return 'case-study';
  }
  return 'tutorial';
}

/** The repository Markdown path for a sheet, or undefined when no such file is shipped.
 * Only a real `@doc_path` header or a DEFAULT_DOC_PATHS entry counts; we no longer synthesise a
 * `docs/<folder>/<id>.md` path, because that pointed at a file which does not exist and the UI
 * rendered it as a "Repository Markdown" provenance claim. The registry gate (web/tests/registry.mjs)
 * keeps every declared path resolvable. */
export function getDocPathForSheet(sheet: Pick<TutorialSheet, 'id' | 'docPath' | 'category'>): string | undefined {
  return sheet.docPath ?? DEFAULT_DOC_PATHS[sheet.id] ?? undefined;
}

export function getFsmPathForSheet(sheet: Pick<TutorialSheet, 'id' | 'fsmPath' | 'docPath' | 'category'>): string {
  if (sheet.fsmPath) return sheet.fsmPath;
  if (DEFAULT_FSM_PATHS[sheet.id]) return DEFAULT_FSM_PATHS[sheet.id];
  const docPath = getDocPathForSheet(sheet);
  if (docPath) {
    return docPath
      .replace(/^docs\/analytic-tests\//, 'examples/analytic/')
      .replace(/^docs\/tutorials\//, 'examples/tutorials/')
      .replace(/^docs\/case-studies\//, 'examples/case-studies/')
      .replace(/\.md$/, '.fsm');
  }
  // No doc path to derive from: fall back to the category's examples folder.
  const cat = getCategoryForSheet(sheet);
  const folder = cat === 'analytic' ? 'analytic' : cat === 'case-study' ? 'case-studies' : 'tutorials';
  return `examples/${folder}/${sheet.id}.fsm`;
}

export function getVerificationRefsForSheet(sheet: Pick<TutorialSheet, 'id' | 'verificationRefs'>): string[] {
  return sheet.verificationRefs ?? DEFAULT_VERIFICATION_REFS[sheet.id] ?? [];
}

export function getVerificationStatusForSheet(sheet: Pick<TutorialSheet, 'verificationStatus'>): 'verified' | 'pending' {
  return sheet.verificationStatus ?? 'verified';
}

export function getCategoryLabel(cat: TutorialCategory): string {
  switch (cat) {
    case 'analytic':
      return 'Analytic Verification Test';
    case 'case-study':
      return 'Multiphysics Case Study';
    case 'tutorial':
    default:
      return 'Guided Tutorial';
  }
}

const LITERATE_START_MARKER = '# === FIELDSMITH LITERATE CASE ===';
const LITERATE_END_MARKER = '# === END LITERATE HEADER ===';

/** Strip the `# === FIELDSMITH LITERATE CASE === ... # === END LITERATE HEADER ===` comment block
 * to return the concise executable `fieldsmith 1` script. */
export function extractExecutableFsm(rawFsm: string): string {
  const lines = rawFsm.replace(/\r\n/g, '\n').split('\n');
  const startIdx = lines.findIndex((l) => l.trim() === LITERATE_START_MARKER);
  const endIdx = lines.findIndex((l) => l.trim() === LITERATE_END_MARKER);
  if (startIdx < 0 || endIdx < startIdx) {
    return rawFsm;
  }
  const before = lines.slice(0, startIdx);
  let afterStart = endIdx + 1;
  if (afterStart < lines.length && lines[afterStart].trim() === '') {
    afterStart += 1;
  }
  const after = lines.slice(afterStart);
  return [...before, ...after].join('\n');
}

/** Serialise a `TutorialSheet` into a single self-defining Literate `.fsm` file. */
export function formatLiterateFsm(sheet: TutorialSheet): string {
  const cat = getCategoryForSheet(sheet);
  const docPath = getDocPathForSheet(sheet);
  const fsmPath = getFsmPathForSheet(sheet);
  const vRefs = getVerificationRefsForSheet(sheet);

  const rawBody = extractExecutableFsm(sheet.starterFsm).replace(/\r\n/g, '\n');
  const bodyLines = rawBody.split('\n');
  let headerFirstLine = 'fieldsmith 1';
  let restBodyLines = bodyLines;
  if (bodyLines[0]?.trim() === 'fieldsmith 1') {
    headerFirstLine = bodyLines[0];
    restBodyLines = bodyLines.slice(1);
  }

  const out: string[] = [];
  out.push(headerFirstLine);
  out.push(LITERATE_START_MARKER);
  out.push(`# @id: ${sheet.id}`);
  out.push(`# @code: ${sheet.code}`);
  out.push(`# @category: ${cat}`);
  out.push(`# @level: ${sheet.level}`);
  out.push(`# @title: ${sheet.title}`);
  out.push(`# @subtitle: ${sheet.subtitle}`);
  out.push(`# @verification_badge: ${sheet.verificationBadge}`);
  if (sheet.verificationStatus) out.push(`# @verification_status: ${sheet.verificationStatus}`);
  if (docPath) out.push(`# @doc_path: ${docPath}`);
  out.push(`# @fsm_path: ${fsmPath}`);
  out.push(`# @starter_fsm_name: ${sheet.starterFsmName}`);
  if (sheet.opensSystemsWorkbench) {
    out.push(`# @opens_systems_workbench: true`);
  }
  if (vRefs.length > 0) {
    out.push(`# @verification_refs: ${vRefs.join(', ')}`);
  }

  if (sheet.analyticChecks && sheet.analyticChecks.length > 0) {
    out.push('# ---');
    for (const c of sheet.analyticChecks) {
      out.push(
        `# @check: ${c.quantity} || ${c.formula} || ${c.analyticRef} || ${c.measuredFea} || ${c.relError} || ${c.tolerance}`,
      );
    }
  }

  if (sheet.asserts && sheet.asserts.length > 0) {
    out.push('# ---');
    for (const a of sheet.asserts) {
      out.push(`# @assert: ${a.label} || ${a.metric} || ${a.target} || ${a.tolerance} || ${a.unit}`);
    }
  }

  if (sheet.userSteps.length > 0) {
    out.push('# ---');
    for (const st of sheet.userSteps) {
      out.push(`# @step: ${st.title} || ${st.lens ?? ''}`);
      for (const line of st.instruction.split('\n')) {
        out.push(`#   ${line}`);
      }
      out.push('# @expected:');
      for (const line of st.expected.split('\n')) {
        out.push(`#   ${line}`);
      }
    }
  }

  out.push('# ---');
  out.push(`# @theory_title: ${sheet.theoryChapter.chapterTitle}`);
  out.push('# @theory_summary:');
  for (const line of sheet.theoryChapter.summary.split('\n')) {
    out.push(`#   ${line}`);
  }
  for (const sec of sheet.theoryChapter.sections) {
    out.push(`# @theory_section: ${sec.heading}`);
    out.push('# @theory_eq:');
    for (const line of sec.equations.split('\n')) {
      out.push(`#   ${line}`);
    }
    out.push('# @theory_body:');
    for (const line of sec.body.split('\n')) {
      out.push(`#   ${line}`);
    }
  }

  if (sheet.mcpSteps.length > 0) {
    out.push('# ---');
    const cleanStarterTrimmed = rawBody.trim();
    for (const ms of sheet.mcpSteps) {
      const argsCopy: Record<string, unknown> = { ...ms.arguments };
      if (typeof argsCopy.fsm === 'string' && argsCopy.fsm.trim() === cleanStarterTrimmed) {
        argsCopy.fsm = '@SELF';
      }
      out.push(`# @mcp: ${ms.title} || ${ms.tool} || ${JSON.stringify(argsCopy)}`);
      for (const line of ms.description.split('\n')) {
        out.push(`#   ${line}`);
      }
    }
  }

  out.push(LITERATE_END_MARKER);
  out.push('');
  out.push(...restBodyLines);
  return out.join('\n');
}

type BlockMode =
  | 'none'
  | 'step_instruction'
  | 'step_expected'
  | 'theory_summary'
  | 'theory_eq'
  | 'theory_body'
  | 'mcp_description';

/** Parse a Literate `.fsm` file into a full `TutorialSheet`. */
export function parseLiterateFsm(rawFsm: string, fallbackFsmPath?: string): TutorialSheet {
  const normalized = rawFsm.replace(/\r\n/g, '\n');
  const starterFsm = extractExecutableFsm(normalized);
  const lines = normalized.split('\n');
  const startIdx = lines.findIndex((l) => l.trim() === LITERATE_START_MARKER);
  const endIdx = lines.findIndex((l) => l.trim() === LITERATE_END_MARKER);

  let id = 'CUSTOM';
  let code = 'CUSTOM';
  let category: TutorialCategory | undefined;
  let docPath: string | undefined;
  let fsmPath: string | undefined = fallbackFsmPath;
  let verificationRefs: string[] | undefined;
  const analyticChecks: AnalyticCheckRow[] = [];
  const asserts: LiterateAssertRow[] = [];
  let level: TutorialLevel = 'Intermediate';
  let title = 'Untitled Fieldsmith Case';
  let subtitle = '';
  let verificationBadge = 'Fieldsmith Case';
  let verificationStatus: 'verified' | 'pending' | undefined;
  let starterFsmName = 'model.fsm';
  let opensSystemsWorkbench: boolean | undefined;

  const userSteps: TutorialStep[] = [];
  let currentStep: { title: string; lens?: Lens | 'systems'; instructionLines: string[]; expectedLines: string[] } | null =
    null;

  let theoryTitle = '';
  const theorySummaryLines: string[] = [];
  const theorySections: { heading: string; eqLines: string[]; bodyLines: string[] }[] = [];
  let currentTheorySec: { heading: string; eqLines: string[]; bodyLines: string[] } | null = null;

  const mcpSteps: { title: string; tool: string; arguments: Record<string, unknown>; descLines: string[] }[] = [];
  let currentMcp: { title: string; tool: string; arguments: Record<string, unknown>; descLines: string[] } | null = null;

  const flushStep = () => {
    if (currentStep) {
      userSteps.push({
        title: currentStep.title,
        ...(currentStep.lens ? { lens: currentStep.lens } : {}),
        instruction: currentStep.instructionLines.join('\n'),
        expected: currentStep.expectedLines.join('\n'),
      });
      currentStep = null;
    }
  };

  const flushTheorySec = () => {
    if (currentTheorySec) {
      theorySections.push(currentTheorySec);
      currentTheorySec = null;
    }
  };

  const flushMcp = () => {
    if (currentMcp) {
      mcpSteps.push(currentMcp);
      currentMcp = null;
    }
  };

  if (startIdx >= 0 && endIdx > startIdx) {
    let mode: BlockMode = 'none';
    for (let i = startIdx + 1; i < endIdx; i++) {
      const rawLine = lines[i];
      const trimmed = rawLine.trim();
      if (trimmed === '# ---') {
        mode = 'none';
        continue;
      }
      const dirMatch = rawLine.match(/^#\s*@([a-z_]+):(.*)$/);
      if (dirMatch) {
        const tag = dirMatch[1];
        const val = dirMatch[2].trim();
        switch (tag) {
          case 'id':
            id = val;
            mode = 'none';
            break;
          case 'code':
            code = val;
            mode = 'none';
            break;
          case 'category':
            category = val as TutorialCategory;
            mode = 'none';
            break;
          case 'level':
            level = val as TutorialLevel;
            mode = 'none';
            break;
          case 'title':
            title = val;
            mode = 'none';
            break;
          case 'subtitle':
            subtitle = val;
            mode = 'none';
            break;
          case 'verification_badge':
            verificationBadge = val;
            mode = 'none';
            break;
          case 'verification_status':
            verificationStatus = val === 'pending' ? 'pending' : val === 'verified' ? 'verified' : undefined;
            mode = 'none';
            break;
          case 'doc_path':
            docPath = val;
            mode = 'none';
            break;
          case 'fsm_path':
            fsmPath = val;
            mode = 'none';
            break;
          case 'starter_fsm_name':
            starterFsmName = val;
            mode = 'none';
            break;
          case 'opens_systems_workbench':
            opensSystemsWorkbench = val === 'true';
            mode = 'none';
            break;
          case 'verification_refs':
            verificationRefs = val
              .split(',')
              .map((s) => s.trim())
              .filter(Boolean);
            mode = 'none';
            break;
          case 'check': {
            const parts = val.split(' || ');
            analyticChecks.push({
              quantity: parts[0] ?? '',
              formula: parts[1] ?? '',
              analyticRef: parts[2] ?? '',
              measuredFea: parts[3] ?? '',
              relError: parts[4] ?? '',
              tolerance: parts[5] ?? '',
            });
            mode = 'none';
            break;
          }
          case 'assert': {
            const parts = val.split(' || ').map((p) => p.trim());
            asserts.push({
              label: parts[0] ?? '',
              metric: parts[1] ?? '',
              target: parts[2] ?? '',
              tolerance: parts[3] ?? '',
              unit: parts[4] ?? '',
            });
            mode = 'none';
            break;
          }
          case 'step': {
            flushStep();
            const parts = val.split(' || ');
            const stTitle = parts[0] ?? '';
            const stLens = (parts[1] ?? '').trim();
            currentStep = {
              title: stTitle,
              lens: stLens ? (stLens as Lens | 'systems') : undefined,
              instructionLines: [],
              expectedLines: [],
            };
            mode = 'step_instruction';
            break;
          }
          case 'expected':
            mode = 'step_expected';
            break;
          case 'theory_title':
            flushStep();
            theoryTitle = val;
            mode = 'none';
            break;
          case 'theory_summary':
            mode = 'theory_summary';
            break;
          case 'theory_section':
            flushTheorySec();
            currentTheorySec = {
              heading: val,
              eqLines: [],
              bodyLines: [],
            };
            mode = 'none';
            break;
          case 'theory_eq':
            mode = 'theory_eq';
            break;
          case 'theory_body':
            mode = 'theory_body';
            break;
          case 'mcp': {
            flushTheorySec();
            flushMcp();
            const parts = val.split(' || ');
            const mTitle = parts[0] ?? '';
            const mTool = parts[1] ?? '';
            const mArgsRaw = parts.slice(2).join(' || ');
            let parsedArgs: Record<string, unknown> = {};
            try {
              parsedArgs = JSON.parse(mArgsRaw);
            } catch {
              parsedArgs = {};
            }
            if (parsedArgs.fsm === '@SELF') {
              parsedArgs.fsm = starterFsm;
            }
            currentMcp = {
              title: mTitle,
              tool: mTool,
              arguments: parsedArgs,
              descLines: [],
            };
            mode = 'mcp_description';
            break;
          }
          default:
            mode = 'none';
            break;
        }
        continue;
      }

      if (rawLine.startsWith('#') && mode !== 'none') {
        const content = rawLine.startsWith('#   ') ? rawLine.slice(4) : rawLine.slice(1).replace(/^\s{0,3}/, '');
        switch (mode) {
          case 'step_instruction':
            currentStep?.instructionLines.push(content);
            break;
          case 'step_expected':
            currentStep?.expectedLines.push(content);
            break;
          case 'theory_summary':
            theorySummaryLines.push(content);
            break;
          case 'theory_eq':
            currentTheorySec?.eqLines.push(content);
            break;
          case 'theory_body':
            currentTheorySec?.bodyLines.push(content);
            break;
          case 'mcp_description':
            currentMcp?.descLines.push(content);
            break;
        }
      }
    }
    flushStep();
    flushTheorySec();
    flushMcp();
  } else {
    // Fallback for plain `.fsm` files without a literate header
    const nameMatch = normalized.match(/^name\s+"([^"]+)"/m);
    if (nameMatch) title = nameMatch[1];
    if (fallbackFsmPath) {
      const base = fallbackFsmPath.split('/').pop()?.replace(/\.fsm$/, '') ?? 'CUSTOM';
      starterFsmName = `${base}.fsm`;
      const codeMatch = base.match(/^([AT]\d+)/i);
      if (codeMatch) {
        id = codeMatch[1].toUpperCase();
        code = id;
      } else {
        id = base;
        code = base;
      }
    }
  }

  const sheet: TutorialSheet = {
    id,
    code,
    ...(category ? { category } : {}),
    ...(docPath ? { docPath } : {}),
    ...(fsmPath ? { fsmPath } : {}),
    ...(verificationRefs ? { verificationRefs } : {}),
    ...(analyticChecks.length > 0 ? { analyticChecks } : {}),
    ...(asserts.length > 0 ? { asserts } : {}),
    level,
    title,
    subtitle,
    verificationBadge,
    ...(verificationStatus ? { verificationStatus } : {}),
    starterFsmName,
    starterFsm,
    ...(opensSystemsWorkbench ? { opensSystemsWorkbench: true } : {}),
    userSteps,
    theoryChapter: {
      chapterTitle: theoryTitle,
      summary: theorySummaryLines.join('\n'),
      sections: theorySections.map((s) => ({
        heading: s.heading,
        equations: s.eqLines.join('\n'),
        body: s.bodyLines.join('\n'),
      })),
    },
    mcpSteps: mcpSteps.map((m) => ({
      title: m.title,
      description: m.descLines.join('\n'),
      tool: m.tool,
      arguments: m.arguments,
    })),
  };

  return sheet;
}

const RAW_LITERATE_FSM_MODULES = import.meta.glob(
  '../../../examples/{analytic,tutorials,case-studies}/*.fsm',
  {
    query: '?raw',
    import: 'default',
    eager: true,
  },
) as Record<string, string>;

function sheetSortKey(s: TutorialSheet): [number, number, string] {
  const m = s.id.match(/^([TA])(\d+)$/i);
  if (m) {
    const prefixOrder = m[1].toUpperCase() === 'T' ? 0 : 1;
    return [prefixOrder, Number.parseInt(m[2], 10), s.id];
  }
  return [2, 0, s.id];
}

export const TUTORIAL_SHEETS: TutorialSheet[] = Object.entries(RAW_LITERATE_FSM_MODULES)
  .map(([relPath, rawText]) => {
    const normalizedPath = relPath.replace(/^(\.\.\/)+/, '');
    return parseLiterateFsm(rawText, normalizedPath);
  })
  .sort((a, b) => {
    const ka = sheetSortKey(a);
    const kb = sheetSortKey(b);
    if (ka[0] !== kb[0]) return ka[0] - kb[0];
    if (ka[1] !== kb[1]) return ka[1] - kb[1];
    return ka[2].localeCompare(kb[2]);
  });

/** Match an active `Document` (by `doc.name`) to its corresponding Literate `.fsm` sheet, if any. */
export function findMatchingLiterateSheet(docName: string): TutorialSheet | null {
  const trimmed = (docName || '').trim();
  if (!trimmed) return null;
  const prefixMatch = trimmed.match(/^([TA]\d+)\b/i);
  if (prefixMatch) {
    const code = prefixMatch[1].toUpperCase();
    const byCode = TUTORIAL_SHEETS.find((s) => s.id.toUpperCase() === code);
    if (byCode) return byCode;
  }
  const exact = TUTORIAL_SHEETS.find((s) => s.starterFsm.includes(`name "${trimmed}"`));
  return exact ?? null;
}


