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

export interface TutorialSheet {
  id: string;
  code: string;
  category?: TutorialCategory;
  docPath?: string;
  fsmPath?: string;
  verificationRefs?: string[];
  analyticChecks?: AnalyticCheckRow[];
  level: TutorialLevel;
  title: string;
  subtitle: string;
  verificationBadge: string;
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
  T14: ['docs/verification/V53.md'],
  T15: ['docs/verification/V4.md'],
  T16: ['docs/verification/V42.md'],
  T17: ['docs/verification/V60.md', 'docs/verification/V20.md'],
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
  A13: ['docs/verification/V53.md'],
  A14: ['docs/verification/V42.md'],
};

export function getCategoryForSheet(sheet: Pick<TutorialSheet, 'id' | 'category'>): TutorialCategory {
  if (sheet.category) return sheet.category;
  if (sheet.id.startsWith('A')) return 'analytic';
  if (['T8', 'T9', 'T10', 'T11', 'T12', 'T13', 'T16', 'T17'].includes(sheet.id)) {
    return 'case-study';
  }
  return 'tutorial';
}

export function getDocPathForSheet(sheet: Pick<TutorialSheet, 'id' | 'docPath' | 'category'>): string {
  if (sheet.docPath) return sheet.docPath;
  if (DEFAULT_DOC_PATHS[sheet.id]) return DEFAULT_DOC_PATHS[sheet.id];
  const cat = getCategoryForSheet(sheet);
  const folder = cat === 'analytic' ? 'analytic-tests' : cat === 'case-study' ? 'case-studies' : 'tutorials';
  return `docs/${folder}/${sheet.id}.md`;
}

export function getFsmPathForSheet(sheet: Pick<TutorialSheet, 'id' | 'fsmPath' | 'docPath' | 'category'>): string {
  if (sheet.fsmPath) return sheet.fsmPath;
  if (DEFAULT_FSM_PATHS[sheet.id]) return DEFAULT_FSM_PATHS[sheet.id];
  const docPath = getDocPathForSheet(sheet);
  return docPath
    .replace(/^docs\/analytic-tests\//, 'examples/analytic/')
    .replace(/^docs\/tutorials\//, 'examples/tutorials/')
    .replace(/^docs\/case-studies\//, 'examples/case-studies/')
    .replace(/\.md$/, '.fsm');
}

export function getVerificationRefsForSheet(sheet: Pick<TutorialSheet, 'id' | 'verificationRefs'>): string[] {
  return sheet.verificationRefs ?? DEFAULT_VERIFICATION_REFS[sheet.id] ?? [];
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
  out.push(`# @doc_path: ${docPath}`);
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
  let level: TutorialLevel = 'Intermediate';
  let title = 'Untitled Fieldsmith Case';
  let subtitle = '';
  let verificationBadge = 'Fieldsmith Case';
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
    level,
    title,
    subtitle,
    verificationBadge,
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


