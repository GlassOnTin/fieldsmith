// The address grammar `<id>.<attr>[.<k>]` plus bare `let` names (GOAL v0.4 section D),
// mirrored from crates/fieldsmith-doc/src/param.rs: a sweep value edits one number of
// one named statement, or sets a named scalar and recomputes the expressions that read
// it. A bare id is a let address and never a statement name — shapes are swept through
// their attribute addresses, and an attribute a let's expression writes is refused with
// "sweep the let that drives it". Ids resolve against the document — nodes first, then
// loads, then temperatures, then convections, then heat fluxes — and applying a value is an edit of that
// one number.
// Placement (`at`, `rotate`) is addressable on any node; everything else the grammar
// does not name (region selectors, profile vertices, mesh internals, solve settings,
// fill fields) is refused in the error text rather than left to the caller. The error
// strings are the Rust ones word for word, so the web sweep answers with the same
// sentences the native sweep does; numbers reach those texts through rustFloat, the
// Rust-Display twin.

import type { Consumer, Document, Let, Node } from './types';
import { allIds, findNode, walk } from './types';
import { rustFloat } from './numfmt';
import { isWasmLoaded, syncSketchFeatures } from './wasm';

function syncBoundSketches(doc: Document): void {
  if ((doc.sketch_features?.length ?? 0) > 0 && isWasmLoaded()) {
    const synced = syncSketchFeatures(doc);
    doc.root = synced.root;
    doc.sketches = synced.sketches;
    doc.sketch_features = synced.sketch_features;
  }
}

/** One parsed address: let name (attr "") or statement id, attribute name, optional
 * 0-based component index. */
export interface ParamAddr {
  id: string;
  attr: string;
  comp: number | null;
}

/** The full address as written, for error texts and the picker. */
export function addrText(a: ParamAddr): string {
  if (a.attr === '') return a.id;
  return a.comp === null ? `${a.id}.${a.attr}` : `${a.id}.${a.attr}.${a.comp}`;
}

/** Parse `id.attr`, `id.attr.n`, or a bare let name; throws the Rust parse error on
 * anything else. */
export function parseAddr(addr: string): ParamAddr {
  const bad = () =>
    `bad param address ${JSON.stringify(addr)}: expect a let name or id.attr / id.attr.n — e.g. beam.size.2, hole1.radius, pull.force.1, w`;
  // A bare id is a let address; a dot starts the statement-attribute grammar.
  if (!addr.includes('.')) {
    if (isIdent(addr)) return { id: addr, attr: '', comp: null };
    throw new Error(bad());
  }
  const dot = addr.indexOf('.');
  const id = addr.slice(0, dot);
  const rest = addr.slice(dot + 1);
  if (!id || !rest) throw new Error(bad());
  if (!isIdent(id)) {
    // The first-char rule is implied by the whole-id test, but the Rust source checks
    // them separately; the message is the same either way.
    throw new Error(bad());
  }
  const parts = rest.split('.');
  if (parts.length === 1) {
    if (!parts[0]) throw new Error(bad());
    return { id, attr: parts[0], comp: null };
  }
  if (parts.length === 2) {
    // usize::from_str: an optional '+' sign and decimal digits only, no whitespace.
    if (!/^\+?\d+$/.test(parts[1])) throw new Error(bad());
    if (!parts[0]) throw new Error(bad());
    return { id, attr: parts[0], comp: Number(parts[1]) };
  }
  throw new Error(bad());
}

/** The Rust `is_ident`: an ASCII alphabetic or '_' first char, then alphanumerics/'_'. */
function isIdent(s: string): boolean {
  return /^[a-zA-Z_][a-zA-Z0-9_]*$/.test(s);
}

/** The address without its component index, for the "needs a component index" error. */
function withoutComp(a: ParamAddr): string {
  return `${a.id}.${a.attr}`;
}

function needIndex(a: ParamAddr, attr: string): number {
  if (a.comp === null) {
    throw new Error(
      `param ${addrText(a)}: ${attr} needs a component index — ${withoutComp(a)}.0 .. ${withoutComp(a)}.2`,
    );
  }
  return a.comp;
}

function checkIndex(a: ParamAddr, attr: string, k: number): void {
  if (k < 0 || k > 2) {
    throw new Error(`param ${addrText(a)}: ${attr}.${k} is out of range (${withoutComp(a)}.0 .. ${withoutComp(a)}.2)`);
  }
}

function noIndex(a: ParamAddr, attr: string): void {
  if (a.comp !== null) throw new Error(`param ${addrText(a)}: ${attr} takes no component index`);
}

function attrsNote(kind: Node['kind']): string {
  const shape =
    kind.type === 'box'
      ? 'size[0-2] (size.n), round'
      : kind.type === 'cylinder'
        ? 'radius, height, round'
        : kind.type === 'sphere'
          ? 'radius'
          : kind.type === 'capsule'
            ? 'radius, length'
            : kind.type === 'torus'
              ? 'major, minor'
              : kind.type === 'extrude'
                ? 'height (profile vertices are not sweep parameters)'
                : kind.type === 'group'
                  ? 'a group carries no numbers; its shapes do'
                  : kind.type === 'mesh'
                    ? 'a mesh import carries no sweep parameters (its grid was baked at import)'
                    : kind.type === 'extend'
                      ? "an extend's dir/len are fixed at declaration"
                      : 'the node carries no numbers';
  return `${shape} — and at[0-2], rotate[0-2] for the placement of any node`;
}

/** Resolve the address against what the document actually names — a bare id against the
 * let table, a dotted one against nodes first, then loads, then temperatures, then
 * convections — and check the attribute's component rule. No mutation. */
export function resolveAddr(doc: Document, addr: ParamAddr): void {
  const { id, attr } = addr;
  // A bare id is a let address and never a statement name: shapes are swept
  // through their attribute addresses.
  if (attr === '') {
    if ((doc.lets ?? []).some((l) => l.name === id)) return;
    throw new Error(`param ${addrText(addr)}: no parameter named "${id}"`);
  }
  const node = findNode(doc, id);
  if (node) {
    // `at` and `rotate` address any node's placement, meshes and groups included.
    if (attr === 'at' || attr === 'rotate') {
      checkIndex(addr, attr, needIndex(addr, attr));
      return;
    }
    const k = node.kind;
    if (k.type === 'box' && attr === 'size') {
      checkIndex(addr, attr, needIndex(addr, attr));
      return;
    }
    const scalar: Record<string, string[]> = {
      box: ['round'],
      cylinder: ['radius', 'height', 'round'],
      sphere: ['radius'],
      capsule: ['radius', 'length'],
      torus: ['major', 'minor'],
      extrude: ['height'],
      revolve: ['angle'],
    };
    if (scalar[k.type]?.includes(attr)) {
      noIndex(addr, attr);
      return;
    }
    throw new Error(
      `param ${addrText(addr)}: node ${JSON.stringify(id)} (${k.type}) has no numeric parameter ${JSON.stringify(attr)} — ${attrsNote(k)}`,
    );
  }
  const load = doc.loads.find((l) => l.id === id);
  if (load) {
    if (load.kind.type === 'force' && attr === 'force') {
      checkIndex(addr, attr, needIndex(addr, attr));
      return;
    }
    if (
      (load.kind.type === 'pressure' && attr === 'mpa') ||
      (load.kind.type === 'preload' && attr === 'newtons') ||
      (load.kind.type === 'distributed' && attr === 'total')
    ) {
      noIndex(addr, attr);
      return;
    }
    const takes =
      load.kind.type === 'force'
        ? 'force[0-2] (force.n)'
        : load.kind.type === 'pressure'
          ? 'mpa'
          : load.kind.type === 'preload'
            ? 'newtons'
            : 'total';
    throw new Error(
      `param ${addrText(addr)}: load ${JSON.stringify(id)} (${load.kind.type}) has no numeric parameter ${JSON.stringify(attr)} — its parameters are ${takes}`,
    );
  }
  const temp = doc.temperatures.find((t) => t.id === id);
  if (temp) {
    if (attr !== 'celsius') {
      throw new Error(
        `param ${addrText(addr)}: temperature ${JSON.stringify(id)} has no numeric parameter ${JSON.stringify(attr)} — celsius`,
      );
    }
    noIndex(addr, attr);
    return;
  }
  const conv = doc.convections.find((c) => c.id === id);
  if (conv) {
    if (attr !== 'h' && attr !== 'tinf') {
      throw new Error(
        `param ${addrText(addr)}: convection ${JSON.stringify(id)} has no numeric parameter ${JSON.stringify(attr)} — h, tinf`,
      );
    }
    noIndex(addr, attr);
    return;
  }
  const flux = doc.heat_fluxes.find((f) => f.id === id);
  if (flux) {
    if (attr !== 'q') {
      throw new Error(
        `param ${addrText(addr)}: heat flux ${JSON.stringify(id)} has no numeric parameter ${JSON.stringify(attr)} — q`,
      );
    }
    noIndex(addr, attr);
    return;
  }
  const contact = (doc.contacts ?? []).find((c) => c.id === id);
  if (contact) {
    if (attr !== 'k' && attr !== 'gap') {
      throw new Error(
        `param ${addrText(addr)}: contact ${JSON.stringify(id)} has no numeric parameter ${JSON.stringify(attr)} — k, gap`,
      );
    }
    noIndex(addr, attr);
    return;
  }
  if (id === 'fill' && doc.fill && !findNode(doc, 'fill')) {
    if (attr === 'rotate') {
      if (addr.comp === null || addr.comp < 0 || addr.comp > 2) {
        throw new Error(`param ${addrText(addr)}: rotate expects component 0..2`);
      }
      return;
    }
    if (
      [
        'cell',
        'cell_min',
        'skin',
        'skin_max',
        'rho_min',
        'rho_max',
        'curve',
        'min_wall',
        'wall',
      ].includes(attr)
    ) {
      noIndex(addr, attr);
      return;
    }
    throw new Error(
      `param ${addrText(addr)}: fill has no numeric parameter ${JSON.stringify(attr)} — cell, cell_min, skin, skin_max, rho_min, rho_max, curve, min_wall, rotate[0-2]`,
    );
  }
  if (id === 'hx' && doc.hx) {
    if (!['mdot', 'tin', 'cold_mdot', 'cold_tin', 'pout'].includes(attr)) {
      throw new Error(
        `param ${addrText(addr)}: hx has no numeric parameter ${JSON.stringify(attr)} — mdot, tin, cold_mdot, cold_tin, pout`,
      );
    }
    noIndex(addr, attr);
    return;
  }
  throw new Error(
    `param ${addrText(addr)}: no node, load, temperature, convection or heat flux named ${JSON.stringify(id)} (or contact/hx)`,
  );
}

/** Apply one numeric value to the addressed parameter. The address must have resolved
 * against this document (resolve at request time); this re-runs the same checks so a
 * stale caller cannot write outside the grammar. Non-finite values are refused. An
 * attribute a declared expression writes is refused with the sweep-the-let hint; a
 * bare let applies through [`applyLet`], re-evaluating what reads it. */
export function applyAddr(doc: Document, addr: ParamAddr, value: number): void {
  resolveAddr(doc, addr);
  if (!Number.isFinite(value)) {
    throw new Error(`param ${addrText(addr)}: value ${rustFloat(value)} is not finite`);
  }
  if (addr.attr === '') {
    applyLet(doc, addr.id, value);
    return;
  }
  // A number a declared expression writes must be swept through its let: editing it
  // here would be overwritten by the expression that drives it.
  const c = (doc.consumed ?? []).find(
    (c) => c.target === addr.id && c.attr === addr.attr && c.comp === addr.comp,
  );
  if (c) {
    throw new Error(
      `param ${addrText(addr)}: written by an expression ('${c.expr}') — sweep the let that drives it`,
    );
  }
  writePlain(doc, addr.id, addr.attr, addr.comp, value);
  syncBoundSketches(doc);
}

/** Set one or more lets simultaneously and re-evaluate all remaining lets and recorded
 * expressions in declaration order. When `updateLetExpr` is true, also updates `l.expr`
 * on the applied lets so `printFsm` emits the new literal. */
function applyLets(doc: Document, applied: ReadonlyMap<string, number>, updateLetExpr = false): void {
  const lets = doc.lets as Let[];
  for (const [name, value] of applied) {
    const slot = lets.findIndex((l) => l.name === name);
    if (slot < 0) throw new Error(`param ${name}: no let named '${name}'`);
    lets[slot].value = value;
    if (updateLetExpr) lets[slot].expr = rustFloat(value);
  }
  const firstName = applied.keys().next().value ?? '';
  const exprs = lets.map((l) => [l.name, l.expr] as const);
  const values: [string, number][] = [];
  for (const [oname, expr] of exprs) {
    // Applied lets keep their swept values; every other let re-evaluates in
    // declaration order, so derived lets chain. Re-evaluating an applied let
    // itself would clobber the sweep with its const-folded declaration value.
    if (applied.has(oname)) {
      values.push([oname, applied.get(oname)!]);
      continue;
    }
    let v: number;
    try {
      v = evalFsmExpr(expr, values);
    } catch (e) {
      throw new Error(`param ${firstName}: the let '${oname}' does not evaluate: ${(e as Error).message}`);
    }
    if (!Number.isFinite(v)) {
      throw new Error(`param ${firstName}: the let '${oname}' value is not finite`);
    }
    values.push([oname, v]);
  }
  for (let i = 0; i < lets.length; i++) lets[i].value = values[i][1];
  for (const c of (doc.consumed ?? []).slice()) {
    let v: number;
    try {
      v = evalFsmExpr(c.expr, values);
    } catch (e) {
      throw new Error(
        `param ${firstName}: the expression for '${consumerSpec(c)}' does not evaluate: ${(e as Error).message}`,
      );
    }
    if (!Number.isFinite(v)) {
      throw new Error(`param ${firstName}: the expression for '${consumerSpec(c)}' value is not finite`);
    }
    writePlain(doc, c.target, c.attr, c.comp, v);
  }
  syncBoundSketches(doc);
}

/** Set one let and re-evaluate the lets and recorded expressions that read it, in
 * declaration order so derived lets chain. The errors name the applied let. */
function applyLet(doc: Document, name: string, value: number): void {
  applyLets(doc, new Map([[name, value]]), false);
}

/** Apply multiple `(ParamAddr, value)` pairs simultaneously (1D or 2D sweeps and Pareto
 * click-to-apply). When `updateLetExpr` is true, applied `let` statements also update
 * their `expr` literal so `printFsm(doc)` reflects the applied values. */
export function applyAddrs(doc: Document, pairs: readonly (readonly [ParamAddr, number])[], updateLetExpr = false): void {
  const letMap = new Map<string, number>();
  for (const [addr, value] of pairs) {
    resolveAddr(doc, addr);
    if (!Number.isFinite(value)) {
      throw new Error(`param ${addrText(addr)}: value ${rustFloat(value)} is not finite`);
    }
    if (addr.attr === '') {
      letMap.set(addr.id, value);
    } else {
      const c = (doc.consumed ?? []).find(
        (c) => c.target === addr.id && c.attr === addr.attr && c.comp === addr.comp,
      );
      if (c) {
        throw new Error(
          `param ${addrText(addr)}: written by an expression ('${c.expr}') — sweep the let that drives it`,
        );
      }
      writePlain(doc, addr.id, addr.attr, addr.comp, value);
    }
  }
  if (letMap.size > 0) {
    applyLets(doc, letMap, updateLetExpr);
  } else if (pairs.length > 0) {
    syncBoundSketches(doc);
  }
}

/** The dotted spelling of one recorded consumer, for re-evaluation errors. */
function consumerSpec(c: Consumer): string {
  return c.comp === null ? `${c.target}.${c.attr}` : `${c.target}.${c.attr}.${c.comp}`;
}

function syncExtrudeHoleNodes(
  nodes: Document['root'],
  prefix: string,
  heightOpt: number | null,
  translate: [number, number, number],
  rotate: [number, number, number],
): void {
  for (const n of nodes) {
    if (n.id.startsWith(prefix)) {
      const rest = n.id.slice(prefix.length);
      if (/^\d+$/.test(rest) && n.op === 'subtract' && n.kind.type === 'extrude') {
        if (heightOpt !== null) n.kind.height = Math.max(0.5, heightOpt + 1);
        n.transform.translate = [...translate];
        n.transform.rotate = [...rotate];
      }
    }
    if (n.kind.type === 'group') {
      syncExtrudeHoleNodes(n.kind.children, prefix, heightOpt, translate, rotate);
    }
  }
}

/** Write one number into the plain field of the addressed statement. Checked callers
 * only: [`applyAddr`] after resolve, [`applyLet`] for recorded consumers (parse-time
 * recorded, removed with their statement). */
function writePlain(doc: Document, target: string, attr: string, comp: number | null, v: number): void {
  const node = findNode(doc, target);
  if (node) {
    const k = node.kind;
    if (attr === 'at') {
      node.transform.translate[comp as number] = v;
      if (k.type === 'extrude') {
        syncExtrudeHoleNodes(doc.root, `${target}_hole`, null, node.transform.translate, node.transform.rotate);
      }
    } else if (attr === 'rotate') {
      node.transform.rotate[comp as number] = v;
      if (k.type === 'extrude') {
        syncExtrudeHoleNodes(doc.root, `${target}_hole`, null, node.transform.translate, node.transform.rotate);
      }
    } else if (k.type === 'box') {
      if (attr === 'size') k.size[comp as number] = v;
      else k.round = v;
    } else if (k.type === 'cylinder') {
      if (attr === 'radius') k.radius = v;
      else if (attr === 'height') k.height = v;
      else k.round = v;
    } else if (k.type === 'sphere') k.radius = v;
    else if (k.type === 'capsule') {
      if (attr === 'radius') k.radius = v;
      else k.length = v;
    } else if (k.type === 'torus') {
      if (attr === 'major') k.major = v;
      else k.minor = v;
    } else if (k.type === 'extrude') {
      k.height = v;
      for (const sf of doc.sketch_features ?? []) {
        if (sf.feature_id === target) {
          if (sf.operation.kind === 'boss_extrude') sf.operation.height = v;
          else if (sf.operation.kind === 'pocket_cut' && !sf.operation.through_all) {
            sf.operation.depth = Math.max(0.1, v - 0.5);
          }
        }
      }
      syncExtrudeHoleNodes(doc.root, `${target}_hole`, v, node.transform.translate, node.transform.rotate);
    } else if (k.type === 'revolve') {
      k.angle = v;
      for (const sf of doc.sketch_features ?? []) {
        if (
          sf.feature_id === target &&
          (sf.operation.kind === 'revolve_boss' || sf.operation.kind === 'revolve_cut')
        ) {
          sf.operation.angle_deg = v;
        }
      }
    }
    return;
  }
  const load = doc.loads.find((l) => l.id === target);
  if (load) {
    if (load.kind.type === 'force') load.kind.vector[comp as number] = v;
    else if (load.kind.type === 'pressure') load.kind.mpa = v;
    else if (load.kind.type === 'preload') load.kind.newtons = v;
    else load.kind.total = v;
    return;
  }
  const temp = doc.temperatures.find((t) => t.id === target);
  if (temp) temp.celsius = v;
  const conv = doc.convections.find((c) => c.id === target);
  if (conv) {
    if (attr === 'h') conv.h = v;
    else if (attr === 'tinf') conv.t_inf = v;
  }
  const flux = doc.heat_fluxes.find((f) => f.id === target);
  if (flux) flux.q = v;
  const contact = (doc.contacts ?? []).find((c) => c.id === target);
  if (contact) {
    if (attr === 'k') contact.k = v;
    else if (attr === 'gap') contact.gap = v;
    return;
  }
  if (target === 'fill' && doc.fill) {
    if (attr === 'cell') doc.fill.cell = v;
    else if (attr === 'cell_min') doc.fill.cell_min = v;
    else if (attr === 'skin') doc.fill.skin = v;
    else if (attr === 'skin_max') doc.fill.skin_max = v;
    else if (attr === 'rho_min') doc.fill.rho_min = v;
    else if (attr === 'rho_max') doc.fill.rho_max = v;
    else if (attr === 'curve') doc.fill.curve = v;
    else if (attr === 'min_wall' || attr === 'wall') doc.fill.min_wall = v;
    else if (attr === 'rotate') {
      doc.fill.rotate ??= [0, 0, 0];
      doc.fill.rotate[comp as number] = v;
    }
    return;
  }
  if (target === 'hx' && doc.hx) {
    if (attr === 'mdot') doc.hx.mdot = v;
    else if (attr === 'tin') doc.hx.tin = v;
    else if (attr === 'cold_mdot') doc.hx.cold_mdot = v;
    else if (attr === 'cold_tin') doc.hx.cold_tin = v;
    else if (attr === 'pout') doc.hx.pout_kpa = v;
  }
}

// Expression evaluation, the fsm.rs twin --------------------------------------

/** One `.fsm` arithmetic expression in Rust's grammar and fold order: atoms, unary
 * minus, left-associative `*`/`/`, then `+`/`-`. Throws the plain Rust messages; the
 * statement-side callers add the attribute context. Scope pairs name lets in order of
 * their evaluation, matching the Rust `&[(String, f64)]` first-match lookup. */
export function evalFsmExpr(text: string, lets: readonly (readonly [string, number])[]): number {
  const toks = fsmTokens(text);
  fsmCursor = 0; // the cursor is module state shared by one parse at a time — reset per expression
  const tree = fsmSum(toks);
  if (fsmCursor < toks.length) throw new Error('unexpected text after the expression');
  return evalFsmTree(tree, lets);
}

let fsmCursor = 0;

function fsmTokens(text: string): Token[] {
  const chars = [...text];
  const out: Token[] = [];
  let i = 0;
  while (i < chars.length) {
    const c = chars[i];
    // Rust char::is_whitespace: the Unicode White_Space set, which JS '\s' extends
    // with U+FEFF and drops U+0085 from — both differ in the pathological cases only.
    if (/^[\t\n\u000b\u000c\r \u0085\u00a0\u1680\u2000-\u200a\u2028\u2029\u202f\u205f\u3000]$/.test(c)) {
      i++;
    } else if (c === '#') {
      break;
    } else if (c === '"') {
      let s = '';
      i++;
      for (;;) {
        if (i >= chars.length) throw new Error('unterminated string');
        if (chars[i] === '"') {
          i++;
          break;
        }
        if (chars[i] === '\\' && i + 1 < chars.length) {
          s += chars[i + 1];
          i += 2;
        } else {
          s += chars[i];
          i++;
        }
      }
      out.push({ t: 'str', s });
    } else if (
      /[0-9]/.test(c) ||
      ((c === '-' || c === '+' || c === '.') &&
        i + 1 < chars.length &&
        (/[0-9]/.test(chars[i + 1]) || (chars[i + 1] === '.' && c !== '.')) &&
        out[out.length - 1]?.t !== 'ident' &&
        out[out.length - 1]?.t !== 'num')
    ) {
      const start = i;
      i++;
      while (
        i < chars.length &&
        (/[0-9]/.test(chars[i]) ||
          chars[i] === '.' ||
          chars[i] === 'e' ||
          chars[i] === 'E' ||
          ((chars[i] === '-' || chars[i] === '+') && (chars[i - 1] === 'e' || chars[i - 1] === 'E')))
      ) {
        i++;
      }
      const s = chars.slice(start, i).join('');
      const v = Number(s);
      if (Number.isNaN(v)) throw new Error(`bad number '${s}'`);
      out.push({ t: 'num', v });
    } else if (/[a-zA-Z_]/.test(c)) {
      const start = i;
      while (i < chars.length && (/[a-zA-Z0-9_]/.test(chars[i]) || chars[i] === '_')) i++;
      out.push({ t: 'ident', s: chars.slice(start, i).join('') });
    } else if ('(){}[],=.+*/-^'.includes(c)) {
      out.push({ t: 'sym', c });
      i++;
    } else {
      throw new Error(`unexpected character '${c}'`);
    }
  }
  return out;
}

type Token = { t: 'ident'; s: string } | { t: 'num'; v: number } | { t: 'str'; s: string } | { t: 'sym'; c: string };

/** A parsed expression tree — the Rust `Expr` mirrored with its fold order. */
type FsmExpr =
  | { t: 'num'; v: number }
  | { t: 'var'; s: string }
  | { t: 'call'; name: string; args: FsmExpr[] }
  | { t: 'neg'; x: FsmExpr }
  | { t: 'bin'; op: '+' | '-' | '*' | '/' | '^'; l: FsmExpr; r: FsmExpr };

function fsmPeek(toks: Token[]): Token | undefined {
  return toks[fsmCursor];
}
function fsmNext(toks: Token[]): Token | undefined {
  return toks[fsmCursor++];
}

/** Parse an atomic operand: number, identifier, function call `f(a, ...)`, or `(e)`. */
function fsmAtom(toks: Token[]): FsmExpr {
  const n = fsmNext(toks);
  if (!n) throw new Error('expected a value');
  if (n.t === 'num') return { t: 'num', v: n.v };
  if (n.t === 'ident') {
    const p = fsmPeek(toks);
    if (p && p.t === 'sym' && p.c === '(') {
      fsmCursor++;
      const args: FsmExpr[] = [];
      const next = fsmPeek(toks);
      if (!(next && next.t === 'sym' && next.c === ')')) {
        for (;;) {
          args.push(fsmSum(toks));
          const sep = fsmPeek(toks);
          if (sep && sep.t === 'sym' && sep.c === ',') {
            fsmCursor++;
          } else {
            break;
          }
        }
      }
      const close = fsmNext(toks);
      if (!close || !(close.t === 'sym' && close.c === ')')) throw new Error("expected ')'");
      return { t: 'call', name: n.s, args };
    }
    return { t: 'var', s: n.s };
  }
  if (n.t === 'sym' && n.c === '(') {
    const e = fsmSum(toks);
    const close = fsmNext(toks);
    if (!close || !(close.t === 'sym' && close.c === ')')) throw new Error("expected ')'");
    return e;
  }
  throw new Error('expected a value');
}

/** Parse a power expression: `atom ('^' unary)?` (right-associative, binds tighter than unary `-`). */
function fsmPower(toks: Token[]): FsmExpr {
  const a = fsmAtom(toks);
  const t = fsmPeek(toks);
  if (t && t.t === 'sym' && t.c === '^') {
    fsmCursor++;
    return { t: 'bin', op: '^', l: a, r: fsmUnary(toks) };
  }
  return a;
}

/** Parse a unary operand: `-e` or a power expression. */
function fsmUnary(toks: Token[]): FsmExpr {
  const t = fsmPeek(toks);
  if (t && t.t === 'sym' && t.c === '-') {
    fsmCursor++;
    return { t: 'neg', x: fsmUnary(toks) };
  }
  return fsmPower(toks);
}

/** Fold `*` and `/` (left-associative) onto a parsed left operand. */
function fsmProducts(toks: Token[], left: FsmExpr): FsmExpr {
  for (;;) {
    const t = fsmPeek(toks);
    if (!(t && t.t === 'sym' && (t.c === '*' || t.c === '/'))) return left;
    fsmCursor++;
    left = { t: 'bin', op: t.c as '*' | '/', l: left, r: fsmUnary(toks) };
  }
}

/** A product-level operand, for the right side of a sum. */
function fsmProduct(toks: Token[]): FsmExpr {
  return fsmProducts(toks, fsmUnary(toks));
}

/** Continue a parsed left operand through any remaining products, then sums. */
function fsmTail(toks: Token[], left: FsmExpr): FsmExpr {
  left = fsmProducts(toks, left);
  for (;;) {
    const t = fsmPeek(toks);
    if (!(t && t.t === 'sym' && (t.c === '+' || t.c === '-'))) return left;
    fsmCursor++;
    left = { t: 'bin', op: t.c as '+' | '-', l: left, r: fsmProduct(toks) };
  }
}

/** Parse a full expression from the cursor. */
function fsmSum(toks: Token[]): FsmExpr {
  return fsmTail(toks, fsmUnary(toks));
}

function evalFsmTree(e: FsmExpr, lets: readonly (readonly [string, number])[]): number {
  switch (e.t) {
    case 'num':
      return e.v;
    case 'var': {
      for (const [name, v] of lets) {
        if (name === e.s) return v;
      }
      if (e.s === 'pi') return Math.PI;
      if (e.s === 'e') return Math.E;
      throw new Error(`no let named '${e.s}'`);
    }
    case 'call': {
      const vals = e.args.map((a) => evalFsmTree(a, lets));
      const checkArity = (want: number) => {
        if (vals.length !== want) {
          throw new Error(`${e.name} takes ${want} argument${want === 1 ? '' : 's'}, given ${vals.length}`);
        }
      };
      switch (e.name) {
        case 'sin':
          checkArity(1);
          return Math.sin(vals[0]);
        case 'cos':
          checkArity(1);
          return Math.cos(vals[0]);
        case 'tan':
          checkArity(1);
          return Math.tan(vals[0]);
        case 'sqrt':
          checkArity(1);
          return Math.sqrt(vals[0]);
        case 'abs':
          checkArity(1);
          return Math.abs(vals[0]);
        case 'exp':
          checkArity(1);
          return Math.exp(vals[0]);
        case 'ln':
          checkArity(1);
          return Math.log(vals[0]);
        case 'min':
          checkArity(2);
          return Math.min(vals[0], vals[1]);
        case 'max':
          checkArity(2);
          return Math.max(vals[0], vals[1]);
        case 'pow':
          checkArity(2);
          return Math.pow(vals[0], vals[1]);
        default:
          throw new Error(`unknown function '${e.name}'`);
      }
    }
    case 'neg':
      return -evalFsmTree(e.x, lets);
    case 'bin': {
      const lv = evalFsmTree(e.l, lets);
      const rv = evalFsmTree(e.r, lets);
      if (e.op === '+') return lv + rv;
      if (e.op === '-') return lv - rv;
      if (e.op === '*') return lv * rv;
      if (e.op === '^') return Math.pow(lv, rv);
      if (rv === 0.0) throw new Error('division by zero');
      return lv / rv;
    }
  }
}

/** The current value of an addressable parameter, in the document's own units; null
 * where the statement carries no value yet (a distributed load's total). */
export function readParam(doc: Document, addr: ParamAddr): number | null {
  // A bare id reads the let table.
  if (addr.attr === '') {
    return (doc.lets ?? []).find((l) => l.name === addr.id)?.value ?? null;
  }
  const node = findNode(doc, addr.id);
  if (node) {
    const k = node.kind;
    if (addr.attr === 'at') return node.transform.translate[addr.comp as number];
    if (addr.attr === 'rotate') return node.transform.rotate[addr.comp as number];
    if (k.type === 'box') return addr.attr === 'size' ? k.size[addr.comp as number] : k.round;
    if (k.type === 'cylinder') {
      return addr.attr === 'radius' ? k.radius : addr.attr === 'height' ? k.height : k.round;
    }
    if (k.type === 'sphere') return k.radius;
    if (k.type === 'capsule') return addr.attr === 'radius' ? k.radius : k.length;
    if (k.type === 'torus') return addr.attr === 'major' ? k.major : k.minor;
    if (k.type === 'extrude') return k.height;
    if (k.type === 'revolve') return k.angle;
    return null;
  }
  const load = doc.loads.find((l) => l.id === addr.id);
  if (load) {
    if (load.kind.type === 'force') return load.kind.vector[addr.comp as number];
    if (load.kind.type === 'pressure') return load.kind.mpa;
    if (load.kind.type === 'preload') return load.kind.newtons;
    return load.kind.total ?? null;
  }
  const temp = doc.temperatures.find((t) => t.id === addr.id);
  if (temp) return temp.celsius;
  const conv = doc.convections.find((c) => c.id === addr.id);
  if (conv) return addr.attr === 'h' ? conv.h : conv.t_inf;
  const flux = doc.heat_fluxes.find((f) => f.id === addr.id);
  if (flux) return flux.q;
  const contact = (doc.contacts ?? []).find((c) => c.id === addr.id);
  if (contact) return addr.attr === 'k' ? contact.k : contact.gap;
  if (addr.id === 'fill' && doc.fill && !findNode(doc, 'fill')) {
    if (addr.attr === 'cell') return doc.fill.cell;
    if (addr.attr === 'cell_min') return doc.fill.cell_min ?? null;
    if (addr.attr === 'skin') return doc.fill.skin;
    if (addr.attr === 'skin_max') return doc.fill.skin_max ?? null;
    if (addr.attr === 'rho_min') return doc.fill.rho_min;
    if (addr.attr === 'rho_max') return doc.fill.rho_max;
    if (addr.attr === 'curve') return doc.fill.curve;
    if (addr.attr === 'min_wall' || addr.attr === 'wall') return doc.fill.min_wall;
    if (addr.attr === 'rotate') return (doc.fill.rotate ?? [0, 0, 0])[addr.comp as number];
  }
  if (addr.id === 'hx' && doc.hx) {
    if (addr.attr === 'mdot') return doc.hx.mdot;
    if (addr.attr === 'tin') return doc.hx.tin;
    if (addr.attr === 'cold_mdot') return doc.hx.cold_mdot ?? null;
    if (addr.attr === 'cold_tin') return doc.hx.cold_tin;
    if (addr.attr === 'pout') return doc.hx.pout_kpa;
  }
  return null;
}

/** Every addressable parameter of the document, in resolve order — nodes in walk
 * order (kind numbers first, then at/rotate), then loads, then temperatures, then
 * convections, then the heat fluxes, then the lets. This is the picker's list. */
export function addressableParams(doc: Document): { addr: string; value: number | null }[] {
  const out: { addr: string; value: number | null }[] = [];
  const push = (id: string, attr: string, comp: number | null, value: number | null) =>
    out.push({ addr: comp === null ? `${id}.${attr}` : `${id}.${attr}.${comp}`, value });
  const nodeParams = (n: Node) => {
    const k = n.kind;
    if (k.type === 'box') {
      for (let i = 0; i < 3; i++) push(n.id, 'size', i, k.size[i]);
      push(n.id, 'round', null, k.round);
    } else if (k.type === 'cylinder') {
      push(n.id, 'radius', null, k.radius);
      push(n.id, 'height', null, k.height);
      push(n.id, 'round', null, k.round);
    } else if (k.type === 'sphere') push(n.id, 'radius', null, k.radius);
    else if (k.type === 'capsule') {
      push(n.id, 'radius', null, k.radius);
      push(n.id, 'length', null, k.length);
    } else if (k.type === 'torus') {
      push(n.id, 'major', null, k.major);
      push(n.id, 'minor', null, k.minor);
    } else if (k.type === 'extrude') push(n.id, 'height', null, k.height);
    for (let i = 0; i < 3; i++) push(n.id, 'at', i, n.transform.translate[i]);
    for (let i = 0; i < 3; i++) push(n.id, 'rotate', i, n.transform.rotate[i]);
  };
  walk(doc.root, nodeParams);
  for (const l of doc.loads) {
    if (l.kind.type === 'force') {
      for (let i = 0; i < 3; i++) push(l.id, 'force', i, l.kind.vector[i]);
    } else if (l.kind.type === 'pressure') push(l.id, 'mpa', null, l.kind.mpa);
    else if (l.kind.type === 'preload') push(l.id, 'newtons', null, l.kind.newtons);
    else push(l.id, 'total', null, l.kind.total ?? null);
  }
  for (const t of doc.temperatures) push(t.id, 'celsius', null, t.celsius);
  for (const c of doc.convections) {
    push(c.id, 'h', null, c.h);
    push(c.id, 'tinf', null, c.t_inf);
  }
  for (const f of doc.heat_fluxes) push(f.id, 'q', null, f.q);
  for (const c of doc.contacts ?? []) {
    push(c.id, 'k', null, c.k);
    push(c.id, 'gap', null, c.gap);
  }
  if (doc.fill && !findNode(doc, 'fill')) {
    push('fill', 'cell', null, doc.fill.cell);
    if (doc.fill.cell_min != null) push('fill', 'cell_min', null, doc.fill.cell_min);
    push('fill', 'skin', null, doc.fill.skin);
    if (doc.fill.skin_max != null) push('fill', 'skin_max', null, doc.fill.skin_max);
    push('fill', 'rho_min', null, doc.fill.rho_min);
    push('fill', 'rho_max', null, doc.fill.rho_max);
    push('fill', 'curve', null, doc.fill.curve);
    push('fill', 'min_wall', null, doc.fill.min_wall);
    const rot = doc.fill.rotate ?? [0, 0, 0];
    for (let i = 0; i < 3; i++) push('fill', 'rotate', i, rot[i]);
  }
  if (doc.hx) {
    push('hx', 'mdot', null, doc.hx.mdot);
    push('hx', 'tin', null, doc.hx.tin);
    if (doc.hx.cold_mdot != null) push('hx', 'cold_mdot', null, doc.hx.cold_mdot);
    push('hx', 'cold_tin', null, doc.hx.cold_tin);
    push('hx', 'pout', null, doc.hx.pout_kpa);
  }
  // Lets last, and only their names — a bare id is refused for any statement name, so
  // the two blocks cannot collide.
  for (const l of doc.lets ?? []) out.push({ addr: l.name, value: l.value });
  return out;
}

/** The expression that drives one statement attribute on this document, or null when
 * nothing records it — the Inspector's hint for why a field refuses edits. */
export function drivenExpr(doc: Document, target: string, attr: string, comp: number | null = null): string | null {
  const c = (doc.consumed ?? []).find(
    (c) => c.target === target && c.attr === attr && c.comp === comp,
  );
  return c ? c.expr : null;
}

/** Drop recorded expression reads whose target statement is gone — individual deletes
 * and the region cascade (a deleted region removes its loads and temperatures, whose
 * consumers go with them). */
export function pruneConsumers(doc: Document, gone: Set<string>): void {
  doc.consumed = (doc.consumed ?? []).filter((c) => !gone.has(c.target));
}

const RESERVED_EXPR_IDENTS = new Set([
  'pi',
  'e',
  'sin',
  'cos',
  'tan',
  'sqrt',
  'abs',
  'exp',
  'ln',
  'min',
  'max',
  'pow',
]);

/** Ordered `(name, value)` scope of `doc.lets` for expression evaluation. */
export function letScope(doc: Document): [string, number][] {
  return (doc.lets ?? []).map((l) => [l.name, l.value]);
}

/** Whether `text` references at least one identifier declared in `doc.lets`. */
export function exprReferencesLet(doc: Document, text: string): boolean {
  const names = new Set((doc.lets ?? []).map((l) => l.name));
  if (names.size === 0) return false;
  try {
    const toks = fsmTokens(text);
    return toks.some((t) => t.t === 'ident' && names.has(t.s));
  } catch {
    return false;
  }
}

/** Re-evaluate all `doc.lets` in declaration order and propagate `doc.consumed`. */
export function recomputeAllLets(doc: Document): void {
  const lets = doc.lets ?? [];
  const values: [string, number][] = [];
  for (const l of lets) {
    const v = evalFsmExpr(l.expr, values);
    if (!Number.isFinite(v)) {
      throw new Error(`let '${l.name}' value is not finite`);
    }
    l.value = v;
    values.push([l.name, v]);
  }
  for (const c of (doc.consumed ?? []).slice()) {
    const v = evalFsmExpr(c.expr, values);
    if (!Number.isFinite(v)) {
      throw new Error(`expression for '${consumerSpec(c)}' value is not finite`);
    }
    writePlain(doc, c.target, c.attr, c.comp, v);
  }
  syncBoundSketches(doc);
}

function replaceIdentInExpr(expr: string, from: string, to: string): string {
  const esc = from.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return expr.replace(new RegExp(`\\b${esc}\\b`, 'g'), to);
}

/** Rename a `let` parameter across `doc.lets`, `doc.consumed`, and `doc.uncertainties`.
 * Returns an error message on invalid/colliding name, or `null` on success. */
export function renameLet(doc: Document, from: string, to: string): string | null {
  const next = to.trim();
  if (!next || next === from) return null;
  if (!isIdent(next)) {
    return `Parameter name "${next}" must start with a letter or '_' and use only letters, digits, or '_'.`;
  }
  if (RESERVED_EXPR_IDENTS.has(next)) {
    return `"${next}" is a reserved math identifier in .fsm expressions.`;
  }
  const lets = doc.lets ?? [];
  if (lets.some((l) => l.name === next) || allIds(doc).has(next)) {
    return `Name "${next}" is already in use in this document.`;
  }
  const slot = lets.find((l) => l.name === from);
  if (!slot) return `No parameter named "${from}".`;
  slot.name = next;
  for (const l of lets) {
    l.expr = replaceIdentInExpr(l.expr, from, next);
  }
  for (const c of doc.consumed ?? []) {
    c.expr = replaceIdentInExpr(c.expr, from, next);
  }
  for (const u of doc.uncertainties ?? []) {
    if (u.param === from) u.param = next;
  }
  recomputeAllLets(doc);
  return null;
}

/** Delete a `let` parameter, unbinding any dependent `doc.consumed` entries (keeping their
 * current evaluated numeric values) and removing matching `doc.uncertainties` entries. */
export function deleteLet(doc: Document, name: string): void {
  const esc = name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const re = new RegExp(`\\b${esc}\\b`);
  doc.lets = (doc.lets ?? []).filter((l) => l.name !== name);
  for (const l of doc.lets) {
    if (re.test(l.expr)) {
      l.expr = rustFloat(l.value);
    }
  }
  doc.consumed = (doc.consumed ?? []).filter((c) => !re.test(c.expr));
  if (doc.uncertainties) {
    doc.uncertainties = doc.uncertainties.filter((u) => u.param !== name);
  }
  recomputeAllLets(doc);
}

/** Bind or update a `Consumer` expression on `(target, attr, comp)` and return the evaluated
 * display-unit value (`scale = 0.5` converts Diameter input expressions to `radius = expr / 2`). */
export function upsertConsumerExpr(
  doc: Document,
  target: string,
  attr: string,
  comp: number | null,
  rawInput: string,
  scale = 1,
): number {
  const clean = rawInput.trim().replace(/^=\s*/, '');
  if (!clean) throw new Error('Expression cannot be empty');
  const scope = letScope(doc);
  const displayVal = evalFsmExpr(clean, scope);
  if (!Number.isFinite(displayVal)) throw new Error('Expression value is not finite');
  const simpleAtom = /^[a-zA-Z_][a-zA-Z0-9_]*$/.test(clean);
  const storedExpr =
    scale === 1
      ? clean
      : scale === 0.5
        ? simpleAtom
          ? `${clean} / 2`
          : `(${clean}) / 2`
        : simpleAtom
          ? `${clean} * ${scale}`
          : `(${clean}) * ${scale}`;
  const storedVal = evalFsmExpr(storedExpr, scope);
  if (!Number.isFinite(storedVal)) throw new Error('Scaled expression value is not finite');
  const list = doc.consumed ?? (doc.consumed = []);
  const existing = list.find((c) => c.target === target && c.attr === attr && c.comp === comp);
  if (existing) existing.expr = storedExpr;
  else list.push({ target, attr, comp, expr: storedExpr });
  return displayVal;
}

/** Unlink any `Consumer` expression driving `(target, attr, comp)`. */
export function removeConsumerExpr(
  doc: Document,
  target: string,
  attr: string,
  comp: number | null = null,
): void {
  if (!doc.consumed) return;
  doc.consumed = doc.consumed.filter(
    (c) => !(c.target === target && c.attr === attr && c.comp === comp),
  );
}
