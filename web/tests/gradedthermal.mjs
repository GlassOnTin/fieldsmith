// Two-domain conduction through the graded fill (VISION §7 item 3): with a fill on the
// document, the thermal statements' conduction solve runs on the fill's per-element
// conductivity map — solid elements at k = 1 (the pinned unit), lattice and void elements
// at their sampled material fraction φ (floored at 1e-6), from `conductivity_of` in
// fill.rs. The expansion re-solve and the stress recovery run on the graded moduli, so
// the thermal stress reads the actual core. Boundary statements keep the unit-solid
// reading (`temperature temp=`, `convection h=`, `heatflux q=` are per unit solid k = 1),
// exactly as the solid path documents them.
//
// The native anchors live in fieldsmith-solve's thermal tests (the two-layer series
// resistance cases) and fieldsmith-core's `graded_thermal_flow_goes_through_the_core`
// (the graded dissipation drops below the solid's by the quadratic-form bound). This
// family pins the wasm bridge on the 40 × 10 × 10 bar with a graded gyroid-sheet core:
//
//   solid path — the thermal case still rides `meta.cases` with its nodal field;
//   graded path — the mechanical `cases` carry no thermal entry, the case moves to
//   `meta.thermal_cases`, and `thermalTemperature` delivers the field at exactly the
//   mesh's node count with both faces pinned (100 / 0 °C).
//
// The fixture does not assert a specific interior profile: with a lattice core whose φ
// map is mesh-dependent, the exact field is the native tests' business. What is asserted
// here is the field's existence, its node alignment, the Dirichlet carries, that the
// graded field differs from the solid run (the core's k < 1 does bend the profile), and
// the empty-length contracts on the indices that carry no field.
//
// Run with node tests/run-e2e.mjs --only gradedthermal.
import { writeFileSync } from 'node:fs';
import { resolve } from 'node:path';

const DOC_A = `\
fieldsmith 1
name "Graded thermal bar"
material petg
box beam size=(40, 10, 10) at=(20, 5, 5)
region hot = beam.nx
region cool = beam.px
support clamp hot fixed=xyz
load pull cool force=(10, 0, 0)
temperature hot hot temp=100 case="Thermal 1"
temperature cold cool temp=0
fill gyroid sheet cell=4 skin=1 rho=(0.2, 0.6)
solve size=8 tref=20
`;

/** The wasm results for DOC_FSM: both handles live at once, read into plain arrays
 * (page.evaluate serialises plain data only — the field arrays come back this way). */
async function readHandles(fsmSource) {
  const w = await import('/src/doc/wasm.ts');
  await w.loadWasm();
  const r = w.parseFsm(fsmSource);
  if (!r.ok) throw new Error(r.error?.message ?? 'parse failed');
  const doc = JSON.parse(JSON.stringify(r.ok));
  const solid = w.analyse(doc, { h: 8 });
  const graded = w.analyseGraded(doc, { h: 8 });
  try {
    const solidMeta = JSON.parse(solid.metaJson());
    const gradedMeta = JSON.parse(graded.metaJson());
    const ciSolid = solidMeta.cases.findIndex((c) => c.name === 'Thermal 1');
    const other = ciSolid === 0 ? 1 : 0;
    const readTemp = (out, ci) => Array.from(out.temperature(ci));
    return {
      nodeCount: solid.nodes().length / 3,
      solid: {
        caseIndex: ciSolid,
        thermalField: ciSolid >= 0 ? readTemp(solid, ciSolid) : [],
        mechanicalField: other <= solidMeta.cases.length - 1 ? readTemp(solid, other) : null,
        meta: solidMeta,
      },
      graded: {
        thermal_cases: gradedMeta.thermal_cases ?? null,
        caseNames: gradedMeta.cases.map((c) => c.name),
        mechanicalField: readTemp(graded, 0),
        thermalField: Array.from(graded.thermalTemperature(0)),
        outOfRange: graded.thermalTemperature(7).length,
        meta: gradedMeta,
      },
    };
  } finally {
    solid.free();
    graded.free();
  }
}

export async function gradedthermalTest(browser, url, { screens, docsDir }) {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  const errors = [];
  page.on('console', (m) => {
    if (m.type() === 'error') errors.push(m.text());
  });
  page.on('pageerror', (e) => errors.push(String(e)));
  const checks = [];
  const check = (name, pass, detail = '') => checks.push({ name, pass: !!pass, detail });
  const shot = (name) => resolve(screens, `gradedthermal-${name}.png`);
  const table = (names) =>
    [
      '| Check | Result | Detail |',
      '| :-- | :-- | :-- |',
      ...names.map((name) => {
        const ck = checks.find((x) => x.name === name);
        return `| ${name} | ${ck?.pass ? 'pass' : '**fail**'} | ${ck?.detail ?? ''} |`.replace(/\n/g, ' ');
      }),
    ].join('\n');

  try {
    await page.goto(url);
    await page.evaluate(() => localStorage.clear());
    await page.reload();

    const h = await page.evaluate(readHandles, DOC_A);
    await page.screenshot({ path: shot('01-graded-thermal-wasm') });

    const solid = h.solid;
    check(
      'A: the solid path still puts the thermal case in cases with its nodal field',
      solid.caseIndex >= 0 && solid.thermalField.length === h.nodeCount
        && (solid.meta.cases[solid.caseIndex].temperature ?? []).length === h.nodeCount
        && solid.mechanicalField.length === 0,
      `solid case index ${solid.caseIndex}, field ${solid.thermalField.length} values at ${h.nodeCount} nodes, mechanical-case field ${solid.mechanicalField.length}`,
    );

    const tc = h.graded.thermal_cases ?? [];
    check(
      'A: the graded meta carries the thermal case and cases stays mechanical',
      tc.length === 1 && tc[0].name === 'Thermal 1' && tc[0].conduction?.converged === true
        && tc[0].solve?.converged === true
        && !h.graded.caseNames.includes('Thermal 1')
        && h.graded.mechanicalField.length === 0,
      `thermal_cases [${tc.map((c) => c.name).join(', ')}], conduction converged ${tc[0]?.conduction?.converged ?? 'n/a'}, cases [${h.graded.caseNames.join(', ')}], graded temperature(0) length ${h.graded.mechanicalField.length}`,
    );

    const field = h.graded.thermalField;
    const maxT = field.length ? Math.max(...field) : NaN;
    const minT = field.length ? Math.min(...field) : NaN;
    check(
      'A: thermalTemperature delivers the field at the mesh node count with both faces pinned',
      field.length === h.nodeCount && Math.abs(maxT - 100) < 1e-6 && Math.abs(minT - 0) < 1e-6,
      `${field.length} values at ${h.nodeCount} nodes, range ${minT?.toFixed?.(4)}…${maxT?.toFixed?.(4)} °C`,
    );

    // The core bends the profile: with the lattice's k < 1 the drop moves into the core,
    // so the graded field cannot coincide with the solid run's.
    let maxDelta = 0;
    for (let i = 0; i < Math.min(field.length, solid.thermalField.length); i++) {
      maxDelta = Math.max(maxDelta, Math.abs(field[i] - solid.thermalField[i]));
    }
    check(
      'A: the graded field differs from the solid field — the core bends the profile',
      field.length === solid.thermalField.length && maxDelta > 1e-3,
      `max |graded − solid| = ${maxDelta.toExponential(3)} °C over ${field.length} nodes`,
    );

    check(
      'A: indices with no field read empty — the empty-length contract',
      h.graded.outOfRange === 0,
      `thermalTemperature(7).length ${h.graded.outOfRange} (one thermal case)`,
    );

    check('no console errors', errors.length === 0, errors.join(' | '));
    const done = checks.every((c) => c.pass);
    const passed = checks.filter((c) => c.pass).length;

    const lines = [
      '# V32 — Two-domain conduction through the graded fill',
      '',
      `**Result: ${done ? 'PASS' : 'FAIL'}**  ·  ${passed}/${checks.length} checks  ·  ${new Date().toISOString()}`,
      '',
      'With a fill on the document, the thermal statements re-solve through the lattice',
      'core instead of the solid: conduction runs on the fill\'s per-element conductivity',
      'map (solid elements at k = 1 on the pinned unit solid, lattice and void at their',
      'sampled material fraction φ, voids floored at 1e-6 so their rows stay solvable),',
      'and the expansion re-solve + stress recovery use the graded moduli. The boundary',
      'statements keep the unit-solid reading, as the solid path documents them.',
      '',
      'The native anchors are the solver tests (two-layer series resistance, bit-exact',
      'all-ones assembly) and fieldsmith-core\'s',
      '`graded_thermal_flow_goes_through_the_core` (solid dissipation reproduces the',
      'analytic 25000, the graded dissipation drops below it by the quadratic-form',
      'bound). The browser run pins the wasm bridge: where the case lives, its field',
      'shape, the Dirichlet carries and the empty-length contracts.',
      '',
      '```fsm',
      DOC_A.trimEnd(),
      '```',
      '',
      table([
        'A: the solid path still puts the thermal case in cases with its nodal field',
        'A: the graded meta carries the thermal case and cases stays mechanical',
        'A: thermalTemperature delivers the field at the mesh node count with both faces pinned',
        'A: the graded field differs from the solid field — the core bends the profile',
        'A: indices with no field read empty — the empty-length contract',
        'no console errors',
      ]),
      '',
      '## Method',
      '',
      '`web/tests/gradedthermal.mjs`, driven by `node tests/run-e2e.mjs --only',
      'gradedthermal`. The fixtures call the wasm module directly in the page',
      '(`analyse` + `analyseGraded` at h = 8 mm) and compare the returned arrays — the',
      'ladder\'s packed levels do not yet carry a graded thermal field, see Limits.',
      '',
      '## Limits',
      '',
      '- The Results lens does not yet render the graded thermal field: the worker\'s',
      '  packed level has only `meta.cases` slots, which stay mechanical for a fill',
      '  document, so the case is reachable through the wasm getters and',
      '  `meta.thermal_cases` but shows nothing in the UI display. Surfacing it',
      '  (case picker + legend on graded levels) is the named open UI work.',
      '- The conductivity map is the linear-in-φ Voigt-style read; at homogenised mesh',
      '  sizes it over-reads k (the wall tortuosity is under-resolved). No measured',
      '  conduction law exists yet; the solver tests pin the sub-cell behaviour the map',
      '  claims.',
      '- The fixture does not assert an exact interior profile — with a mesh-dependent φ',
      '  map that is the native tests\' business. The asserted carries are the face pins,',
      '  the node-alignment and that the core bends the field at all.',
      '',
      '---',
      '*Generated by `npm run test:e2e` (web/tests/gradedthermal.mjs) — do not edit by hand.*',
      '',
    ];
    writeFileSync(resolve(docsDir, 'V32.md'), lines.join('\n'));
    console.log(`gradedthermal ${done ? 'PASS' : 'FAIL'}`);
    if (!done) {
      for (const c of checks.filter((c) => !c.pass)) console.error(`  ${c.name}: ${c.detail}`);
    }
    if (errors.length) console.error('console errors:', errors);
    return done;
  } catch (e) {
    console.error('failed checks so far:');
    for (const c of checks.filter((c) => !c.pass)) console.error(`  ${c.name}: ${c.detail}`);
    throw e;
  } finally {
    await page.close();
  }
}