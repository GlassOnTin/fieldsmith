// The address grammar `<id>.<attr>[.<k>]` (GOAL v0.4 section D), mirrored from
// crates/fieldsmith-doc/src/param.rs: a sweep value edits one number of one named
// statement. Ids resolve against the document — nodes first, then loads, then
// temperatures — and applying a value is an edit of that one number. Placement (`at`,
// `rotate`) is addressable on any node; everything else the grammar does not name
// (region selectors, profile vertices, mesh internals, solve settings) is refused in
// the error text rather than left to the caller. The error strings are the Rust ones
// word for word, so the web sweep answers with the same sentences the native sweep
// does; numbers reach those texts through rustFloat, the Rust-Display twin.

import type { Document, Node } from './types';
import { findNode, walk } from './types';
import { rustFloat } from './numfmt';

/** One parsed address: statement id, attribute name, optional 0-based component index. */
export interface ParamAddr {
  id: string;
  attr: string;
  comp: number | null;
}

/** The full address as written, for error texts and the picker. */
export function addrText(a: ParamAddr): string {
  return a.comp === null ? `${a.id}.${a.attr}` : `${a.id}.${a.attr}.${a.comp}`;
}

/** Parse `id.attr` or `id.attr.n`; throws the Rust parse error on anything else. */
export function parseAddr(addr: string): ParamAddr {
  const bad = () =>
    `bad param address ${JSON.stringify(addr)}: expect id.attr or id.attr.n — e.g. beam.size.2, hole1.radius, pull.force.1`;
  const dot = addr.indexOf('.');
  if (dot < 0) throw new Error(bad());
  const id = addr.slice(0, dot);
  const rest = addr.slice(dot + 1);
  if (!id || !rest) throw new Error(bad());
  if (!/^[a-zA-Z_][a-zA-Z0-9_]*$/.test(id)) {
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

/** Resolve the address against what the document actually names — nodes first, then
 * loads, then temperatures — and check the attribute's component rule. No mutation. */
export function resolveAddr(doc: Document, addr: ParamAddr): void {
  const { id, attr } = addr;
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
 * stale caller cannot write outside the grammar. Non-finite values are refused. */
export function applyAddr(doc: Document, addr: ParamAddr, value: number): void {
  resolveAddr(doc, addr);
  if (!Number.isFinite(value)) {
    throw new Error(`param ${addrText(addr)}: value ${rustFloat(value)} is not finite`);
  }
  const node = findNode(doc, addr.id);
  if (node) {
    if (addr.attr === 'at') node.transform.translate[addr.comp as number] = value;
    else if (addr.attr === 'rotate') node.transform.rotate[addr.comp as number] = value;
    else {
      const k = node.kind;
      if (k.type === 'box') {
        if (addr.attr === 'size') k.size[addr.comp as number] = value;
        else k.round = value;
      } else if (k.type === 'cylinder') {
        if (addr.attr === 'radius') k.radius = value;
        else if (addr.attr === 'height') k.height = value;
        else k.round = value;
      } else if (k.type === 'sphere') k.radius = value;
      else if (k.type === 'capsule') {
        if (addr.attr === 'radius') k.radius = value;
        else k.length = value;
      } else if (k.type === 'torus') {
        if (addr.attr === 'major') k.major = value;
        else k.minor = value;
      } else if (k.type === 'extrude') k.height = value;
    }
    return;
  }
  const load = doc.loads.find((l) => l.id === addr.id);
  if (load) {
    if (load.kind.type === 'force') load.kind.vector[addr.comp as number] = value;
    else if (load.kind.type === 'pressure') load.kind.mpa = value;
    else load.kind.total = value;
    return;
  }
  const temp = doc.temperatures.find((t) => t.id === addr.id);
  if (temp) temp.celsius = value;
}

/** The current value of an addressable parameter, in the document's own units; null
 * where the statement carries no value yet (a distributed load's total). */
export function readParam(doc: Document, addr: ParamAddr): number | null {
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
 * order (kind numbers first, then at/rotate), then loads, then temperatures. This is
 * the picker's list. */
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
  return out;
}