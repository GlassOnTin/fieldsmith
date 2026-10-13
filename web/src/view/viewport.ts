// The 3D viewport: a ray-marched field (GLSL generated from the document) plus three.js overlays.
import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import type { Vec3 } from '../doc/types';

const VERT = /* glsl */ `
in vec3 position;
out vec2 v_ndc;
void main() {
  v_ndc = position.xy;
  gl_Position = vec4(position.xy, 0.0, 1.0);
}`;

export const MAX_HIGHLIGHTS = 16;
/** Per-leaf colour cap; beyond the first 32 (tree order) parts render in the base colour. */
export const MAX_PARTCOLORS = 32;

function fragSource(field: string, ident: string): string {
  const active = field.includes('vec4 fs_core(vec3 p) {')
    ? 'vec4 fs_active(vec3 p) { if (u_core > 0.5) return fs_core(p); return fs_field(p); }'
    : 'vec4 fs_active(vec3 p) { return fs_field(p); }';
  return /* glsl */ `
precision highp float;
precision highp int;
precision highp sampler3D;
uniform mat4 u_invViewProj;
uniform mat4 u_viewProj;
uniform vec3 u_lo;
uniform vec3 u_hi;
uniform float u_pixel;      // world size of one pixel at unit distance
uniform float u_step;       // sphere-tracing step scale (< 1 for inexact fields)
uniform vec3 u_base;
uniform vec3 u_camRight;
uniform vec3 u_camUp;
uniform float u_ghost;      // 1 = draw as translucent ghost
uniform float u_alpha;      // body alpha (1 = opaque); the interior pass is opaque
uniform float u_core;       // 1 = march/colour through the interior field (core pass)
uniform vec3 u_leafCol[${MAX_PARTCOLORS}];  // per-leaf display colour (linear RGB)
uniform int u_leafN;
uniform vec3 u_gridCol;     // floor line colour (shared; dark mode flips it)
uniform float u_gridOn;     // 0 hides the floor (shared)
uniform vec2 u_gridCenter;  // fade centre = document centre
uniform vec2 u_gridFade;    // fade radius (inner, outer) in mm from u_gridCenter
uniform vec2 u_hlKey[${MAX_HIGHLIGHTS}];  // (leaf index, face code or -1 for every face)
uniform vec3 u_hlCol[${MAX_HIGHLIGHTS}];
uniform int u_hlN;
in vec2 v_ndc;
out vec4 fragColor;
${field}
${ident}
${active}
bool boxHit(vec3 ro, vec3 rd, out float t0, out float t1) {
  vec3 inv = 1.0 / rd;
  vec3 a = (u_lo - ro) * inv;
  vec3 b = (u_hi - ro) * inv;
  vec3 mn = min(a, b);
  vec3 mx = max(a, b);
  t0 = max(max(mn.x, mn.y), mn.z);
  t1 = min(min(mx.x, mx.y), mx.z);
  return t1 > max(t0, 0.0);
}

// Floor grid: drawn only where the ray misses the part, so it can never appear
// over the surface, from any camera side. Line layout matches the old GridHelper
// (5 mm minor, 50 mm major, 400 mm extent); both families fade past the
// document's neighbourhood so larger documents get a proportionally wider
// clear region. Writes true depth so the axis lines sort against it.
// Coverage is alpha in the fragment itself (three.js uses premultipliedAlpha).
float floorLines(float d, float px, float spacing) {
// Drop lines that alias (spacing under ~2 px) instead of letting them moire.
// spacing/px is small when lines are resolvable, so keep the low end.
  float res = smoothstep(1.5, 3.0, spacing / max(px, 1e-6));
  float aa = 0.7 * px + 1e-5;
  return (1.0 - smoothstep(0.0, aa, d)) * res;
}
bool floorFragment(vec3 ro, vec3 rd, out vec4 col, out float depthOut) {
  if (u_gridOn < 0.5 || u_ghost > 0.5 || u_core > 0.5) return false;
  if (abs(rd.z) < 1e-5) return false;
  float tG = -ro.z / rd.z;
  if (tG <= 0.0) return false;
  vec3 pG = ro + rd * tG;
  if (abs(pG.x) > 200.0 || abs(pG.y) > 200.0) return false;
  float fx = fwidth(pG.x);
  float fy = fwidth(pG.y);
  vec2 dm = abs(fract(pG.xy / 5.0) - 0.5) * 5.0;
  vec2 dM = abs(fract(pG.xy / 50.0) - 0.5) * 50.0;
  float fade = 1.0 - smoothstep(u_gridFade.x, u_gridFade.y, length(pG.xy - u_gridCenter));
  float aMinor = max(floorLines(dm.x, fx, 5.0), floorLines(dm.y, fy, 5.0));
  float aMajor = max(floorLines(dM.x, fx, 50.0), floorLines(dM.y, fy, 50.0));
  float a = max(aMinor * 0.06, aMajor * 0.12) * fade;
  if (a < 0.004) return false;
  vec4 clip = u_viewProj * vec4(pG, 1.0);
  if (clip.w <= 0.0) return false;
  depthOut = clamp(clip.z / clip.w * 0.5 + 0.5, 0.0, 1.0);
  col = vec4(u_gridCol * a, a);
  return true;
}
void main() {
  vec4 a = u_invViewProj * vec4(v_ndc, -1.0, 1.0); a /= a.w;
  vec4 b = u_invViewProj * vec4(v_ndc, 1.0, 1.0); b /= b.w;
  vec3 ro = a.xyz;
  vec3 rd = normalize(b.xyz - a.xyz);
  float tmax = length(b.xyz - a.xyz);
  float t0, t1;
  vec4 floorCol;
  float floorDepth;
  if (!boxHit(ro, rd, t0, t1)) {
    // Ray never enters the part's bounds: only the floor can be visible here.
    if (!floorFragment(ro, rd, floorCol, floorDepth)) discard;
    gl_FragDepth = floorDepth;
    fragColor = floorCol;
    return;
  }
  float t = max(t0, 0.0);
  t1 = min(t1, tmax);
  vec4 f = vec4(1e9);
  bool hit = false;
  float eps = 0.0;
  float tPrev = t;
  for (int i = 0; i < 300; i++) {
    vec3 p = ro + rd * t;
    f = fs_active(p);
    eps = max(1e-4, t * u_pixel * 0.6);
    if (f.x < eps) {
      // A step that lands deep inside material overshot the surface (the field is not
      // exactly Lipschitz where a fillet ends). Bisect back to the zero crossing.
      if (f.x < -eps && i > 0) {
        float lo = tPrev, hi = t;
        for (int j = 0; j < 10; j++) {
          float mid = 0.5 * (lo + hi);
          vec4 fm = fs_active(ro + rd * mid);
          if (fm.x < 0.0) { hi = mid; f = fm; } else lo = mid;
        }
        t = hi;
      }
      hit = true;
      break;
    }
    tPrev = t;
    t += max(f.x * u_step, eps * 0.5);
    if (t > t1) break;
  }
  if (!hit) {
    // Marched inside the bounds but never converged on a surface.
    if (!floorFragment(ro, rd, floorCol, floorDepth)) discard;
    gl_FragDepth = floorDepth;
    fragColor = floorCol;
    return;
  }
  vec3 p = ro + rd * t;
  vec3 n = normalize(f.yzw);
  if (dot(n, rd) > 0.0) n = -n;
  // Leaf identity is queried only when somebody will read it (part colours or highlights),
  // so the no-colour/ no-selection path costs nothing.
  vec2 id = vec2(-1.0, 0.0);
  if (u_leafN > 0 || u_hlN > 0) id = fs_ident(p);
  vec3 base = u_base;
  if (u_leafN > 0) {
    for (int i = 0; i < ${MAX_PARTCOLORS}; i++) {
      if (i >= u_leafN) break;
      if (abs(id.x - float(i)) < 0.5) { base = u_leafCol[i]; break; }
    }
  }
  // Studio lighting: camera-relative key light, sky/ground hemisphere, soft rim.
  vec3 L = normalize(u_camRight * 0.45 + u_camUp * 0.65 - rd * 0.6);
  float diff = max(dot(n, L), 0.0);
  float hemi = 0.5 + 0.5 * n.z;
  float ao = 1.0;
  float h = max(0.6, t * u_pixel * 6.0);
  for (int k = 1; k <= 4; k++) {
    float s = h * float(k);
    ao -= max(s - fs_active(p + n * s).x, 0.0) / s * 0.12;
  }
  ao = clamp(ao, 0.35, 1.0);
  vec3 col = base * (0.30 + 0.22 * hemi + 0.58 * diff) * ao;
  float spec = pow(max(dot(reflect(rd, n), L), 0.0), 48.0) * 0.18;
  float rim = pow(1.0 - max(dot(-rd, n), 0.0), 3.0) * 0.10;
  col += vec3(spec + rim);
  if (u_hlN > 0) {
    for (int i = 0; i < ${MAX_HIGHLIGHTS}; i++) {
      if (i >= u_hlN) break;
      vec2 key = u_hlKey[i];
      if (abs(id.x - key.x) < 0.5 && (key.y < 0.0 || abs(id.y - key.y) < 0.5)) {
        col = mix(col, u_hlCol[i] * (0.55 + 0.45 * diff + 0.25 * hemi), 0.72);
      }
    }
  }
  vec4 clip = u_viewProj * vec4(p, 1.0);
  gl_FragDepth = clamp(clip.z / clip.w * 0.5 + 0.5, 0.0, 1.0);
  fragColor = vec4(pow(col, vec3(1.0 / 2.2)), u_ghost > 0.5 ? 0.28 : u_alpha);
}`;
}

/** One mesh leaf's voxel grid, resolved by the host from the session grid registry. The
 * generated shader bakes origin and spacing as literals, so only the texture payload travels. */
export interface MeshSlotSpec {
  /** Slot in the generated source: binds `u_mesh{slot}`, plus `u_mesh{slot}p` when `patch` is set. */
  slot: number;
  dist: Float32Array;
  patch: Uint16Array | null;
  dims: [number, number, number];
}

export interface FieldSpec {
  source: string;
  bounds: [Vec3, Vec3] | null;
  /** `vec2 fs_ident(vec3 p)`; omitted for ghosts. */
  ident?: string;
  /** Lattice parameter grid for filled parts. */
  grid?: { origin: Vec3; spacing: number; dims: [number, number, number]; data: Float32Array } | null;
  /** Voxel grids for mesh leaves, in the slots the generated source names. */
  meshes?: MeshSlotSpec[];
  stepScale?: number;
}

const NO_IDENT = 'vec2 fs_ident(vec3 p) { return vec2(-1.0, 0.0); }';

/** A surface highlight: every face (face = -1) or one face of the leaf at `leaf`. */
export interface Highlight {
  leaf: number;
  face: number;
  color: [number, number, number];
}

export class Viewport {
  readonly renderer: THREE.WebGLRenderer;
  readonly scene = new THREE.Scene();
  readonly camera: THREE.PerspectiveCamera;
  readonly controls: OrbitControls;
  /** Scene group for overlays (glyphs, result meshes) owned by other modules. */
  readonly overlays = new THREE.Group();
  private quad: THREE.Mesh;
  private ghostQuad: THREE.Mesh;
  // Interior (core) pass of a filled part: opaque, drawn between the floor and the
  // body quad so a semi-transparent body reveals the lattice — only ever mounted.
  private coreQuad: THREE.Mesh;
  private coreMaterial: THREE.RawShaderMaterial | null = null;
  // Floor-only pass for the lens/empty-document states without a field shader
  // (results lens, empty doc). Same floorFragment code, trivial always-miss field.
  private floorQuad: THREE.Mesh;
  private floorMaterial: THREE.RawShaderMaterial | null = null;
  private material: THREE.RawShaderMaterial | null = null;
  private ghostMaterial: THREE.RawShaderMaterial | null = null;
  private grid: THREE.Group;
  // Shared floor-grid uniform objects: every field material aliases these, so a
  // colour flip or toggle updates all of them at once.
  private gridCol = { value: new THREE.Color(0x000000) };
  private gridOn = { value: 1 };
  private dirty = true;
  private interacting = false;
  private idleTimer = 0;
  private frameTimes: number[] = [];
  /** Dispose the 3D textures a retired material owns (u_param, mesh voxel grids). Textures
   * travel inside the uniforms, so drop these before replacing the material — disposing the
   * material alone leaves the GPU resources alive until the next explicit drop. */
  private dropFieldTextures(mat: THREE.RawShaderMaterial | null) {
    if (!mat) return;
    for (const u of Object.values(mat.uniforms)) {
      if (u.value instanceof THREE.Data3DTexture) u.value.dispose();
    }
  }
  bounds: [Vec3, Vec3] | null = null;
  showField = true;
  onFrame: (() => void) | null = null;
  readonly canvas: HTMLCanvasElement;

  constructor(private host: HTMLElement) {
    this.renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true, preserveDrawingBuffer: true });
    this.renderer.setPixelRatio(window.devicePixelRatio);
    this.renderer.setClearColor(0x000000, 0);
    this.canvas = this.renderer.domElement;
    this.canvas.className = 'viewport-canvas';
    host.appendChild(this.canvas);

    this.camera = new THREE.PerspectiveCamera(35, 1, 0.5, 5000);
    this.camera.up.set(0, 0, 1);
    this.camera.position.set(140, -160, 120);
    this.controls = new OrbitControls(this.camera, this.canvas);
    this.controls.enableDamping = true;
    this.controls.dampingFactor = 0.18;
    this.controls.zoomToCursor = true;
    this.controls.mouseButtons = { LEFT: THREE.MOUSE.ROTATE, MIDDLE: THREE.MOUSE.PAN, RIGHT: THREE.MOUSE.PAN };
    this.controls.addEventListener('change', () => this.invalidate(true));
    this.controls.addEventListener('start', () => (this.interacting = true));
    this.controls.addEventListener('end', () => this.endInteraction());

    const geom = new THREE.PlaneGeometry(2, 2);
    this.quad = new THREE.Mesh(geom, new THREE.MeshBasicMaterial({ visible: false }));
    this.quad.frustumCulled = false;
    this.quad.renderOrder = -10;
    this.coreQuad = new THREE.Mesh(geom, new THREE.MeshBasicMaterial({ visible: false }));
    this.coreQuad.frustumCulled = false;
    this.coreQuad.renderOrder = -15;
    this.ghostQuad = new THREE.Mesh(geom, new THREE.MeshBasicMaterial({ visible: false }));
    this.ghostQuad.frustumCulled = false;
    this.ghostQuad.renderOrder = 10;
    this.floorQuad = new THREE.Mesh(geom, new THREE.MeshBasicMaterial({ visible: false }));
    this.floorQuad.frustumCulled = false;
    this.floorQuad.renderOrder = -20;
    this.scene.add(this.quad, this.coreQuad, this.ghostQuad, this.floorQuad);
    this.floorMaterial = this.buildMaterial(
      { source: 'vec4 fs_field(vec3 p) { return vec4(1.0, 0.0, 0.0, 0.0); }', bounds: null, stepScale: 1.0 },
      {});
    this.floorQuad.material = this.floorMaterial;

    this.grid = this.makeGrid();
    this.scene.add(this.grid);
    this.scene.add(this.overlays);
    this.scene.add(new THREE.HemisphereLight(0xffffff, 0x8a8a90, 1.6));
    const key = new THREE.DirectionalLight(0xffffff, 1.4);
    key.position.set(1, -1.2, 2);
    this.scene.add(key);

    new ResizeObserver(() => this.resize()).observe(host);
    this.resize();
    const loop = () => {
      requestAnimationFrame(loop);
      this.controls.update();
      if (this.dirty) {
        this.dirty = false;
        const t0 = performance.now();
        this.render();
        this.frameTimes.push(performance.now() - t0);
        if (this.frameTimes.length > 60) this.frameTimes.shift();
        this.onFrame?.();
      }
    };
    loop();
  }

  private makeGrid(): THREE.Group {
    // The lattice lives in the field shaders (floorFragment); only the small
    // origin axes remain as scene geometry. They sit 0.05 mm above the z=0 plane
    // (far below one screen pixel at working zoom) so they cannot interpenetrate
    // the coplanar part faces or the floor the way same-depth geometry does, and
    // the triad is shifted 2 mm so axis lines are never coplanar with a part
    // face: axis lines coincident with a part's silhouette edges tint those
    // pixels from corner-aligned camera poses (measured 96-172 px).
    const g = new THREE.Group();
    const axes = new THREE.Group();
    axes.position.set(-2, -2, 0);
    const axis = (dir: Vec3, color: number) => {
      const geo = new THREE.BufferGeometry().setFromPoints([
        new THREE.Vector3(0, 0, 0.05),
        new THREE.Vector3(...dir).multiplyScalar(20).setZ(0.05 + dir[2] * 20),
      ]);
      const line = new THREE.Line(geo, new THREE.LineBasicMaterial({ color, transparent: true, opacity: 0.7 }));
      axes.add(line);
    };
    axis([1, 0, 0], 0xd0453b);
    axis([0, 1, 0], 0x3f9b4a);
    axis([0, 0, 1], 0x3b6fd0);
    g.add(axes);
    return g;
  }

  setGridColor(dark: boolean) {
    this.gridCol.value.setHex(dark ? 0xffffff : 0x000000);
    this.invalidate();
  }

  /** Show or hide the floor grid (lattice + axes). */
  toggleGrid(on: boolean) {
    this.gridOn.value = on ? 1 : 0;
    this.invalidate();
  }

  private resize() {
    const w = Math.max(1, this.host.clientWidth);
    const h = Math.max(1, this.host.clientHeight);
    this.renderer.setSize(w, h, false);
    this.camera.aspect = w / h;
    this.camera.updateProjectionMatrix();
    this.invalidate();
  }

  invalidate(fromInteraction = false) {
    this.dirty = true;
    if (fromInteraction) {
      this.interacting = true;
      clearTimeout(this.idleTimer);
      this.idleTimer = window.setTimeout(() => this.endInteraction(), 160);
    }
  }

  private endInteraction() {
    this.interacting = false;
    this.invalidate();
  }

  /** Average render time of recent frames (ms). */
  frameTime(): number {
    if (!this.frameTimes.length) return 0;
    return this.frameTimes.reduce((a, b) => a + b, 0) / this.frameTimes.length;
  }

  private buildMaterial(
    spec: FieldSpec,
    opts: { ghost?: boolean; core?: boolean; translucent?: boolean } = {},
  ): THREE.RawShaderMaterial {
    const ghost = opts.ghost === true;
    const core = opts.core === true;
    const translucent = opts.translucent === true;
    const uniforms: Record<string, THREE.IUniform> = {
      u_invViewProj: { value: new THREE.Matrix4() },
      u_viewProj: { value: new THREE.Matrix4() },
      u_lo: { value: new THREE.Vector3() },
      u_hi: { value: new THREE.Vector3() },
      u_pixel: { value: 0.001 },
      u_step: { value: spec.stepScale ?? 0.9 },
      u_base: { value: new THREE.Color(...(ghost ? ([0.45, 0.6, 0.85] as const) : this.baseColor)) },
      u_camRight: { value: new THREE.Vector3() },
      u_camUp: { value: new THREE.Vector3() },
      u_ghost: { value: ghost ? 1 : 0 },
      u_alpha: { value: ghost ? 0.28 : this.bodyAlpha },
      u_core: { value: core ? 1 : 0 },
      u_gridCol: this.gridCol,
      u_gridOn: this.gridOn,
      u_gridCenter: { value: new THREE.Vector2() },
      u_gridFade: { value: new THREE.Vector2(90, 153) },
      u_leafCol: { value: Array.from({ length: MAX_PARTCOLORS }, () => new THREE.Vector3()) },
      u_leafN: { value: 0 },
      u_hlKey: { value: Array.from({ length: MAX_HIGHLIGHTS }, () => new THREE.Vector2()) },
      u_hlCol: { value: Array.from({ length: MAX_HIGHLIGHTS }, () => new THREE.Vector3()) },
      u_hlN: { value: 0 },
    };
    if (spec.grid && spec.source.includes('u_param')) {
      const g = spec.grid;
      const tex = new THREE.Data3DTexture(g.data, g.dims[0], g.dims[1], g.dims[2]);
      tex.format = THREE.RedFormat;
      tex.type = THREE.FloatType;
      tex.minFilter = THREE.NearestFilter;
      tex.magFilter = THREE.NearestFilter;
      tex.unpackAlignment = 1;
      tex.needsUpdate = true;
      uniforms.u_param = { value: tex };
    }
    // Mesh leaves: the host resolved each slot's grid from the session registry. The patch
    // sampler is u16 in the document so it rides an R16UI integer texture. Slots without a
    // doc-side mesh bind nothing; an unbound usampler reads 0 in WebGL2, which no patch id.
    for (const s of spec.meshes ?? []) {
      const dist = new THREE.Data3DTexture(s.dist, s.dims[0], s.dims[1], s.dims[2]);
      dist.format = THREE.RedFormat;
      dist.type = THREE.FloatType;
      dist.minFilter = THREE.NearestFilter;
      dist.magFilter = THREE.NearestFilter;
      dist.unpackAlignment = 1;
      dist.needsUpdate = true;
      uniforms[`u_mesh${s.slot}`] = { value: dist };
      if (s.patch) {
        const patch = new THREE.Data3DTexture(s.patch, s.dims[0], s.dims[1], s.dims[2]);
        patch.format = THREE.RedIntegerFormat;
        patch.type = THREE.UnsignedShortType;
        patch.minFilter = THREE.NearestFilter;
        patch.magFilter = THREE.NearestFilter;
        patch.unpackAlignment = 1;
        patch.needsUpdate = true;
        uniforms[`u_mesh${s.slot}p`] = { value: patch };
      }
    }
    return new THREE.RawShaderMaterial({
      glslVersion: THREE.GLSL3,
      vertexShader: VERT,
      fragmentShader: fragSource(spec.source, ghost ? NO_IDENT : spec.ident || NO_IDENT),
      uniforms,
      depthTest: true,
      depthWrite: !ghost && !translucent,
      transparent: ghost || translucent,
    });
  }

  private fieldGen = 0;
  private highlights: Highlight[] = [];
  /** Milliseconds the last shader took from source to first use (compile included). */
  lastCompileMs = 0;

  /**
   * Install a new field shader. The program is compiled asynchronously (KHR_parallel_shader_compile
   * where available) and swapped in when ready, so edits never stall the main thread on a compile.
   */
  async setField(spec: FieldSpec): Promise<void> {
    const gen = ++this.fieldGen;
    const t0 = performance.now();
    this.lastSpec = spec;
    if (!spec.bounds) {
      this.dropFieldTextures(this.material);
      this.material?.dispose();
      this.material = null;
      this.bounds = null;
      this.quad.material = new THREE.MeshBasicMaterial({ visible: false });
      this.invalidate();
      return;
    }
    const mat = this.buildMaterial(spec, { translucent: this.bodyAlpha < 1 });
    const probe = new THREE.Mesh(this.quad.geometry, mat);
    probe.frustumCulled = false;
    try {
      await this.renderer.compileAsync(probe, this.camera, this.scene);
    } catch {
      /* fall back to synchronous compile on first render */
    }
    if (gen !== this.fieldGen) {
      this.dropFieldTextures(mat);
      mat.dispose();
      return;
    }
    this.dropFieldTextures(this.material);
    this.material?.dispose();
    this.material = mat;
    this.bounds = spec.bounds;
    this.quad.material = mat;
    this.applyHighlights();
    this.applyPartColors();
    this.lastCompileMs = performance.now() - t0;
    this.invalidate();
  }

  /** Surface highlights (selection, hover, regions); cheap — uniforms only. */
  setHighlights(h: Highlight[]) {
    this.highlights = h.slice(0, MAX_HIGHLIGHTS);
    this.applyHighlights();
    this.invalidate();
  }

  private applyHighlights() {
    const m = this.material;
    if (!m) return;
    const keys = m.uniforms.u_hlKey.value as THREE.Vector2[];
    const cols = m.uniforms.u_hlCol.value as THREE.Vector3[];
    this.highlights.forEach((h, i) => {
      keys[i].set(h.leaf, h.face);
      cols[i].set(...h.color);
    });
    m.uniforms.u_hlN.value = this.highlights.length;
  }

  /** Per-part display colours: id → CSS hex. Filled in on the shader swap and re-applied
   * to whichever materials exist. */
  private partColors: Record<string, string> = {};
  private partLeaves: string[] = [];
  /** The last spec handed to `setField`; the alpha regime flip rebuilds from it. */
  private lastSpec: FieldSpec | null = null;
  /** App-wide body alpha (1 = opaque). The interior pass shows when this is < 1. */
  private bodyAlpha = 1;

  /** The interior (core) pass of a filled document; `null` hides it (no fill, or bounds
   * pending). The spec carries the core pass's own step scale (lower than the body's —
   * the lattice field over-estimates distances). Mirrors `setGhost`. */
  setCore(spec: FieldSpec | null) {
    this.dropFieldTextures(this.coreMaterial);
    this.coreMaterial?.dispose();
    if (!spec || !spec.bounds) {
      this.coreMaterial = null;
      this.coreQuad.material = new THREE.MeshBasicMaterial({ visible: false });
    } else {
      this.coreMaterial = this.buildMaterial(spec, { core: true });
      this.coreQuad.material = this.coreMaterial;
      this.applyPartColors();
    }
    this.invalidate();
  }

  /** Body alpha (0–1). Below 1 the body renders translucent (no depth writes) and the
   * interior pass mounts underneath it. Rebuilding the material happens only on the
   * opaque↔translucent regime flip — the program cache serves the identical source — so a
   * drag inside one regime is a uniform write. Returns the flip's rebuild promise when one
   * was started, so callers can observe `lastCompileMs` after it (a uniform-write change
   * resolves with nothing to observe). */
  setAlpha(a: number): Promise<void> | void {
    const next = THREE.MathUtils.clamp(a, 0, 1);
    const flip = (this.bodyAlpha >= 1) !== (next >= 1) && !!this.lastSpec?.bounds;
    this.bodyAlpha = next;
    const done = flip ? this.setField(this.lastSpec!) : undefined;
    if (!flip && this.material) {
      (this.material.uniforms.u_alpha.value as number) = next;
    }
    this.invalidate();
    return done;
  }

  /** Swap the bound `u_param` payload on both field materials (the analysis's graded grid
   * arrived). Returns false when no material carries `u_param` or `dims` do not match the
   * spec they were built with — the caller must then run a full `setField` with the new
   * grid instead. */
  updateParamGrid(data: Float32Array, dims: [number, number, number]): boolean {
    const g = this.lastSpec?.grid;
    if (!g || g.dims[0] !== dims[0] || g.dims[1] !== dims[1] || g.dims[2] !== dims[2]) return false;
    const materials = [this.material, this.coreMaterial]
      .filter((m): m is THREE.RawShaderMaterial => m !== null && 'u_param' in m.uniforms);
    if (!materials.length) return false;
    for (const m of materials) {
      const tex = new THREE.Data3DTexture(data, dims[0], dims[1], dims[2]);
      tex.format = THREE.RedFormat;
      tex.type = THREE.FloatType;
      tex.minFilter = THREE.NearestFilter;
      tex.magFilter = THREE.NearestFilter;
      tex.unpackAlignment = 1;
      tex.needsUpdate = true;
      (m.uniforms.u_param.value as THREE.Data3DTexture | undefined)?.dispose();
      m.uniforms.u_param.value = tex;
    }
    this.invalidate();
    return true;
  }

  /** Per-part display colours: id → CSS hex, mapped through `leaves` (the fs_ident index
   * order from the body shader). First 32 in `colors`' insertion order (tree order) ride;
   * the rest render in the base colour. */
  setPartColors(colors: Record<string, string>, leaves: string[]) {
    this.partColors = colors;
    this.partLeaves = leaves;
    this.applyPartColors();
    this.invalidate();
  }

  private applyPartColors() {
    // Slots are addressed by the leaf's fs_ident index, not densified: the shader picks
    // `u_leafCol[id.x]` for the fragment's leaf. `u_leafN` must therefore cover the last
    // coloured slot within the first MAX_PARTCOLORS leaves; uncoloured slots still inside
    // that span hold the base colour (a match is a no-op), so a removed colour never
    // leaves a stale value behind. No coloured leaf within the span keeps u_leafN at 0 —
    // the shader then never queries fs_ident for colours.
    const covered = Math.min(this.partLeaves.length, MAX_PARTCOLORS);
    let n = 0;
    for (let i = 0; i < covered; i++) if (this.partColors[this.partLeaves[i]]) n = i + 1;
    for (const m of [this.material, this.coreMaterial]) {
      if (!m) continue;
      const cols = m.uniforms.u_leafCol.value as THREE.Vector3[];
      for (let i = 0; i < n; i++) {
        const css = this.partColors[this.partLeaves[i]];
        if (css) {
          const c = new THREE.Color().setStyle(css); // sRGB → linear working colour space
          cols[i].set(c.r, c.g, c.b);
        } else {
          cols[i].set(...this.baseColor);
        }
      }
      m.uniforms.u_leafN.value = n;
    }
  }

  /** Base surface colour (linear RGB). */
  setBaseColor(c: [number, number, number]) {
    this.baseColor = c;
    if (this.material) (this.material.uniforms.u_base.value as THREE.Color).setRGB(...c);
    // Uncoloured leaf slots hold copies of the base colour; keep them in step.
    this.applyPartColors();
    this.invalidate();
  }
  private baseColor: [number, number, number] = [0.74, 0.73, 0.7];

  /** Translucent ghost of a shape (e.g. a selected cutter); `null` hides it. */
  setGhost(spec: FieldSpec | null) {
    this.dropFieldTextures(this.ghostMaterial);
    this.ghostMaterial?.dispose();
    if (!spec || !spec.bounds) {
      this.ghostMaterial = null;
      this.ghostQuad.material = new THREE.MeshBasicMaterial({ visible: false });
    } else {
      this.ghostMaterial = this.buildMaterial(spec, { ghost: true });
      this.ghostMaterial.userData.bounds = spec.bounds;
      this.ghostQuad.material = this.ghostMaterial;
    }
    this.invalidate();
  }

  private updateUniforms(mat: THREE.RawShaderMaterial, bounds: [Vec3, Vec3]) {
    const cam = this.camera;
    const vp = new THREE.Matrix4().multiplyMatrices(cam.projectionMatrix, cam.matrixWorldInverse);
    mat.uniforms.u_viewProj.value.copy(vp);
    mat.uniforms.u_invViewProj.value.copy(vp).invert();
    mat.uniforms.u_lo.value.set(bounds[0][0] - 0.05, bounds[0][1] - 0.05, bounds[0][2] - 0.05);
    mat.uniforms.u_hi.value.set(bounds[1][0] + 0.05, bounds[1][1] + 0.05, bounds[1][2] + 0.05);
    const uLo = mat.uniforms.u_lo.value as THREE.Vector3;
    const uHi = mat.uniforms.u_hi.value as THREE.Vector3;
    (mat.uniforms.u_gridCenter.value as THREE.Vector2).set(
      0.5 * (uLo.x + uHi.x), 0.5 * (uLo.y + uHi.y));
    // Fade radius from the document's bounds, floor at 90/153 (the default look
    // for a document-sized bracket) so small documents still get a full floor.
    const diag = Math.max(90, Math.hypot(uHi.x - uLo.x, uHi.y - uLo.y, uHi.z - uLo.z));
    (mat.uniforms.u_gridFade.value as THREE.Vector2).set(diag, diag * 1.7);
    const h = this.renderer.domElement.height;
    mat.uniforms.u_pixel.value = (2 * Math.tan(THREE.MathUtils.degToRad(cam.fov) / 2)) / Math.max(1, h);
    const right = new THREE.Vector3().setFromMatrixColumn(cam.matrixWorld, 0);
    const up = new THREE.Vector3().setFromMatrixColumn(cam.matrixWorld, 1);
    mat.uniforms.u_camRight.value.copy(right);
    mat.uniforms.u_camUp.value.copy(up);
  }

  private render() {
    // Render at reduced resolution while the camera moves; full resolution when idle.
    const dpr = window.devicePixelRatio;
    const target = this.interacting ? Math.max(0.5, dpr * 0.5) : dpr;
    if (this.renderer.getPixelRatio() !== target) {
      this.renderer.setPixelRatio(target);
      this.renderer.setSize(this.host.clientWidth, this.host.clientHeight, false);
    }
    this.camera.updateMatrixWorld();
    if (this.material && this.bounds) this.updateUniforms(this.material, this.bounds);
    if (this.coreMaterial && this.bounds) this.updateUniforms(this.coreMaterial, this.bounds);
    if (this.ghostMaterial) this.updateUniforms(this.ghostMaterial, this.ghostMaterial.userData.bounds);
    this.quad.visible = this.showField;
    // The interior pass shows only while the body is semi-transparent: with an opaque
    // body it would be fully hidden anyway and its marching would be pure cost.
    this.coreQuad.visible = this.showField && this.coreMaterial !== null && this.bodyAlpha < 1;
    // The lattice draws in the field shader whenever a field is visible; the
    // results lens (showField false) and empty documents use the floor-only pass.
    const floorVisible = !(this.showField && this.material) && this.gridOn.value > 0.5;
    this.floorQuad.visible = floorVisible;
    if (floorVisible && this.floorMaterial) {
      this.updateUniforms(this.floorMaterial, [[-0.06, -0.06, -0.06], [0.06, 0.06, 0.06]]);
      // fade around the document centre whenever one is loaded, else the origin
      const centre = this.material
        ? (this.material.uniforms.u_gridCenter.value as THREE.Vector2)
        : new THREE.Vector2(0, 0);
      (this.floorMaterial.uniforms.u_gridCenter.value as THREE.Vector2).copy(centre);
    }
    this.grid.visible = this.gridOn.value > 0.5;
    this.renderer.render(this.scene, this.camera);
  }

  /** Frame the given bounds (or the field bounds). */
  fit(bounds = this.bounds, animate = false) {
    if (!bounds) return;
    const lo = new THREE.Vector3(...bounds[0]);
    const hi = new THREE.Vector3(...bounds[1]);
    const centre = lo.clone().add(hi).multiplyScalar(0.5);
    const radius = Math.max(5, hi.clone().sub(lo).length() * 0.5);
    const dir = this.camera.position.clone().sub(this.controls.target).normalize();
    if (dir.lengthSq() < 0.5) dir.set(0.6, -0.7, 0.45).normalize();
    const vfov = THREE.MathUtils.degToRad(this.camera.fov);
    const hfov = 2 * Math.atan(Math.tan(vfov / 2) * this.camera.aspect);
    const dist = (radius / Math.sin(Math.min(vfov, hfov) / 2)) * 1.05;
    const to = centre.clone().add(dir.multiplyScalar(dist));
    if (!animate) {
      this.controls.target.copy(centre);
      this.camera.position.copy(to);
    } else {
      const fromP = this.camera.position.clone();
      const fromT = this.controls.target.clone();
      const start = performance.now();
      const step = () => {
        const k = Math.min(1, (performance.now() - start) / 260);
        const e = 1 - Math.pow(1 - k, 3);
        this.camera.position.lerpVectors(fromP, to, e);
        this.controls.target.lerpVectors(fromT, centre, e);
        this.invalidate(true);
        if (k < 1) requestAnimationFrame(step);
      };
      step();
    }
    this.camera.near = Math.max(0.05, dist / 500);
    this.camera.far = dist * 50;
    this.camera.updateProjectionMatrix();
    this.invalidate();
  }

  /** Orient the camera along a standard view direction, keeping the target. */
  view(dir: Vec3, up: Vec3 = [0, 0, 1]) {
    const dist = this.camera.position.distanceTo(this.controls.target);
    this.camera.up.set(...up);
    this.camera.position.copy(this.controls.target).add(new THREE.Vector3(...dir).normalize().multiplyScalar(dist));
    this.camera.lookAt(this.controls.target);
    this.camera.up.set(0, 0, 1);
    this.invalidate(true);
  }

  /** World-space ray through a client pixel. */
  ray(clientX: number, clientY: number): { origin: Vec3; dir: Vec3 } {
    const r = this.canvas.getBoundingClientRect();
    const ndc = new THREE.Vector2(((clientX - r.left) / r.width) * 2 - 1, -((clientY - r.top) / r.height) * 2 + 1);
    const rc = new THREE.Raycaster();
    rc.setFromCamera(ndc, this.camera);
    return { origin: rc.ray.origin.toArray() as Vec3, dir: rc.ray.direction.toArray() as Vec3 };
  }

  /** Project a world point to client pixels. */
  project(p: Vec3): { x: number; y: number; visible: boolean } {
    const v = new THREE.Vector3(...p).project(this.camera);
    const r = this.canvas.getBoundingClientRect();
    return { x: r.left + ((v.x + 1) / 2) * r.width, y: r.top + ((1 - v.y) / 2) * r.height, visible: v.z < 1 };
  }
}
