// Rigid transforms of nodes in world space (matching fieldsmith-field: R = Rz·Ry·Rx, degrees).
import * as THREE from 'three';
import type { Document, Node, Transform, Vec3 } from './types';

export function localMatrix(t: Transform): THREE.Matrix4 {
  const r = t.rotate.map((d) => THREE.MathUtils.degToRad(d));
  const q = new THREE.Quaternion().setFromEuler(new THREE.Euler(r[0], r[1], r[2], 'ZYX'));
  return new THREE.Matrix4().compose(new THREE.Vector3(...t.translate), q, new THREE.Vector3(1, 1, 1));
}

/** Chain of ancestors (outermost first) for node `id`, or null if absent. */
export function ancestors(doc: Document, id: string): Node[] | null {
  const rec = (list: Node[], path: Node[]): Node[] | null => {
    for (const n of list) {
      if (n.id === id) return path;
      if (n.kind.type === 'group') {
        const r = rec(n.kind.children, [...path, n]);
        if (r) return r;
      }
    }
    return null;
  };
  return rec(doc.root, []);
}

/** World matrix of the space a node's transform is expressed in (its parent's frame). */
export function parentMatrix(doc: Document, id: string): THREE.Matrix4 {
  const m = new THREE.Matrix4();
  for (const a of ancestors(doc, id) ?? []) m.multiply(localMatrix(a.transform));
  return m;
}

export function worldMatrix(doc: Document, node: Node): THREE.Matrix4 {
  return parentMatrix(doc, node.id).multiply(localMatrix(node.transform));
}

/** Decompose a local matrix back into a document transform (rotation in degrees, ZYX order). */
export function toTransform(m: THREE.Matrix4): Transform {
  const p = new THREE.Vector3();
  const q = new THREE.Quaternion();
  const s = new THREE.Vector3();
  m.decompose(p, q, s);
  const e = new THREE.Euler().setFromQuaternion(q, 'ZYX');
  const deg = (x: number) => clean(THREE.MathUtils.radToDeg(x));
  return { translate: [clean(p.x), clean(p.y), clean(p.z)], rotate: [deg(e.x), deg(e.y), deg(e.z)] };
}

/** Round away float noise (1e-9) so documents stay readable. */
export function clean(x: number, digits = 6): number {
  const f = Math.pow(10, digits);
  const r = Math.round(x * f) / f;
  return Object.is(r, -0) ? 0 : r;
}

export function vadd(a: Vec3, b: Vec3): Vec3 {
  return [a[0] + b[0], a[1] + b[1], a[2] + b[2]];
}
