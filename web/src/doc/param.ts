// The address grammar `<id>.<attr>[.<k>]` plus bare `let` names (GOAL v0.4 section D),
// mirrored from crates/fieldsmith-doc/src/param.rs: a sweep value edits one number of
// one named statement, or sets a named scalar and recomputes the expressions that read
// it. A bare id is a let address and never a statement name — shapes are swept through
// their attribute addresses, and an attribute a let's expression writes is refused with
// "sweep the let that drives it". Ids resolve against the document — nodes first, then
// loads, then temperatures — and applying a value is an edit of that one number.
// Placement (`at`, `rotate`) is addressable on any node; everything else the grammar
// does not name (region selectors, profile vertices, mesh internals, solve settings,
// fill fields) is refused in the error text rather than left to the caller. The error
// strings are the Rust ones word for word, so the web sweep answers with the same
// sentences the native sweep does; numbers reach those texts through rustFloat, the
// Rust-Display twin.

import type { Consumer, Document, Let, Node } from './types';
import { findNode, walk } from './types';
import { rustFloat } from './numfmt';

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
 * let table, a dotted one against nodes first, then loads, then temperatures — and
 * check the attribute's component rule. No mutation. */
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
    if ((load.kind.type === 'pressure' && attr === 'mpa') || (load.kind.type === 'distributed' && attr === 'total')) {
      noIndex(addr, attr);
      return;
    }
    const takes = load.kind.type === 'force' ? 'force[0-2] (force.n)' : load.kind.type === 'pressure' ? 'mpa' : 'total';
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
  throw new Error(`param ${addrText(addr)}: no node, load or temperature named ${JSON.stringify(id)}`);
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
}

/** Set one let and re-evaluate the lets and recorded expressions that read it, in
 * declaration order so derived lets chain. The errors name the applied let. */
function applyLet(doc: Document, name: string, value: number): void {
  const lets = doc.lets as Let[];
  const slot = lets.findIndex((l) => l.name === name);
  if (slot < 0) throw new Error(`param ${name}: no let named '${name}'`);
  lets[slot].value = value;
  const exprs = lets.map((l) => [l.name, l.expr] as const);
  const values: [string, number][] = [];
  for (const [oname, expr] of exprs) {
    // The applied let keeps the swept value; every other let re-evaluates in
    // declaration order, so derived lets chain. Re-evaluating the applied let
    // itself would clobber the sweep with its const-folded declaration value.
    if (oname === name) {
      values.push([oname, value]);
      continue;
    }
    let v: number;
    try {
      v = evalFsmExpr(expr, values);
    } catch (e) {
      throw new Error(`param ${name}: the let '${oname}' does not evaluate: ${(e as Error).message}`);
    }
    if (!Number.isFinite(v)) {
      throw new Error(`param ${name}: the let '${oname}' value is not finite`);
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
        `param ${name}: the expression for '${consumerSpec(c)}' does not evaluate: ${(e as Error).message}`,
      );
    }
    if (!Number.isFinite(v)) {
      throw new Error(`param ${name}: the expression for '${consumerSpec(c)}' value is not finite`);
    }
    writePlain(doc, c.target, c.attr, c.comp, v);
  }
}

/** The dotted spelling of one recorded consumer, for re-evaluation errors. */
function consumerSpec(c: Consumer): string {
  return c.comp === null ? `${c.target}.${c.attr}` : `${c.target}.${c.attr}.${c.comp}`;
}

/** Write one number into the plain field of the addressed statement. Checked callers
 * only: [`applyAddr`] after resolve, [`applyLet`] for recorded consumers (parse-time
 * recorded, removed with their statement). */
function writePlain(doc: Document, target: string, attr: string, comp: number | null, v: number): void {
  const node = findNode(doc, target);
  if (node) {
    const k = node.kind;
    if (attr === 'at') node.transform.translate[comp as number] = v;
    else if (attr === 'rotate') node.transform.rotate[comp as number] = v;
    else if (k.type === 'box') {
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
    } else if (k.type === 'extrude') k.height = v;
    return;
  }
  const load = doc.loads.find((l) => l.id === target);
  if (load) {
    if (load.kind.type === 'force') load.kind.vector[comp as number] = v;
    else if (load.kind.type === 'pressure') load.kind.mpa = v;
    else load.kind.total = v;
    return;
  }
  const temp = doc.temperatures.find((t) => t.id === target);
  if (temp) temp.celsius = v;
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
    } else if ('(){}[],=.+*/-'.includes(c)) {
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
  | { t: 'neg'; x: FsmExpr }
  | { t: 'bin'; op: '+' | '-' | '*' | '/'; l: FsmExpr; r: FsmExpr };

function fsmPeek(toks: Token[]): Token | undefined {
  return toks[fsmCursor];
}
function fsmNext(toks: Token[]): Token | undefined {
  return toks[fsmCursor++];
}

/** Parse a unary operand: `-e` or an atom. */
function fsmUnary(toks: Token[]): FsmExpr {
  const t = fsmPeek(toks);
  if (t && t.t === 'sym' && t.c === '-') {
    fsmCursor++;
    return { t: 'neg', x: fsmUnary(toks) };
  }
  const n = fsmNext(toks);
  if (!n) throw new Error('expected a value');
  if (n.t === 'num') return { t: 'num', v: n.v };
  if (n.t === 'ident') return { t: 'var', s: n.s };
  if (n.t === 'sym' && n.c === '(') {
    const e = fsmSum(toks);
    const close = fsmNext(toks);
    if (!close || !(close.t === 'sym' && close.c === ')')) throw new Error("expected ')'");
    return e;
  }
  throw new Error('expected a value');
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
      throw new Error(`no let named '${e.s}'`);
    }
    case 'neg':
      return -evalFsmTree(e.x, lets);
    case 'bin': {
      const lv = evalFsmTree(e.l, lets);
      const rv = evalFsmTree(e.r, lets);
      if (e.op === '+') return lv + rv;
      if (e.op === '-') return lv - rv;
      if (e.op === '*') return lv * rv;
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
    return null;
  }
  const load = doc.loads.find((l) => l.id === addr.id);
  if (load) {
    if (load.kind.type === 'force') return load.kind.vector[addr.comp as number];
    if (load.kind.type === 'pressure') return load.kind.mpa;
    return load.kind.total ?? null;
  }
  const temp = doc.temperatures.find((t) => t.id === addr.id);
  return temp ? temp.celsius : null;
}

/** Every addressable parameter of the document, in resolve order — nodes in walk
 * order (kind numbers first, then at/rotate), then loads, then temperatures, then the
 * lets. This is the picker's list. */
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
    else push(l.id, 'total', null, l.kind.total ?? null);
  }
  for (const t of doc.temperatures) push(t.id, 'celsius', null, t.celsius);
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