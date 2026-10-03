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

function fragSource(field: string, ident: string): string {
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
uniform vec2 u_hlKey[${MAX_HIGHLIGHTS}];  // (leaf index, face code or -1 for every face)
uniform vec3 u_hlCol[${MAX_HIGHLIGHTS}];
uniform int u_hlN;
in vec2 v_ndc;
out vec4 fragColor;
${field}
${ident}
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
void main() {
  vec4 a = u_invViewProj * vec4(v_ndc, -1.0, 1.0); a /= a.w;
  vec4 b = u_invViewProj * vec4(v_ndc, 1.0, 1.0); b /= b.w;
  vec3 ro = a.xyz;
  vec3 rd = normalize(b.xyz - a.xyz);
  float tmax = length(b.xyz - a.xyz);
  float t0, t1;
  if (!boxHit(ro, rd, t0, t1)) discard;
  float t = max(t0, 0.0);
  t1 = min(t1, tmax);
  vec4 f = vec4(1e9);
  bool hit = false;
  float eps = 0.0;
  float tPrev = t;
  for (int i = 0; i < 300; i++) {
    vec3 p = ro + rd * t;
    f = fs_field(p);
    eps = max(1e-4, t * u_pixel * 0.6);
    if (f.x < eps) {
      // A step that lands deep inside material overshot the surface (the field is not
      // exactly Lipschitz where a fillet ends). Bisect back to the zero crossing.
      if (f.x < -eps && i > 0) {
        float lo = tPrev, hi = t;
        for (int j = 0; j < 10; j++) {
          float mid = 0.5 * (lo + hi);
          vec4 fm = fs_field(ro + rd * mid);
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
  if (!hit) discard;
  vec3 p = ro + rd * t;
  vec3 n = normalize(f.yzw);
  if (dot(n, rd) > 0.0) n = -n;
  // Studio lighting: camera-relative key light, sky/ground hemisphere, soft rim.
  vec3 L = normalize(u_camRight * 0.45 + u_camUp * 0.65 - rd * 0.6);
  float diff = max(dot(n, L), 0.0);
  float hemi = 0.5 + 0.5 * n.z;
  float ao = 1.0;
  float h = max(0.6, t * u_pixel * 6.0);
  for (int k = 1; k <= 4; k++) {
    float s = h * float(k);
    ao -= max(s - fs_field(p + n * s).x, 0.0) / s * 0.12;
  }
  ao = clamp(ao, 0.35, 1.0);
  vec3 col = u_base * (0.30 + 0.22 * hemi + 0.58 * diff) * ao;
  float spec = pow(max(dot(reflect(rd, n), L), 0.0), 48.0) * 0.18;
  float rim = pow(1.0 - max(dot(-rd, n), 0.0), 3.0) * 0.10;
  col += vec3(spec + rim);
  if (u_hlN > 0) {
    vec2 id = fs_ident(p);
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
  fragColor = vec4(pow(col, vec3(1.0 / 2.2)), u_ghost > 0.5 ? 0.28 : 1.0);
}`;
}

export interface FieldSpec {
  source: string;
  bounds: [Vec3, Vec3] | null;
  /** `vec2 fs_ident(vec3 p)`; omitted for ghosts. */
  ident?: string;
  /** Lattice parameter grid for filled parts. */
  grid?: { origin: Vec3; spacing: number; dims: [number, number, number]; data: Float32Array } | null;
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
  private material: THREE.RawShaderMaterial | null = null;
  private ghostMaterial: THREE.RawShaderMaterial | null = null;
  private grid: THREE.Group;
  private dirty = true;
  private interacting = false;
  private idleTimer = 0;
  private frameTimes: number[] = [];
  private paramTexture: THREE.Data3DTexture | null = null;
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
    this.ghostQuad = new THREE.Mesh(geom, new THREE.MeshBasicMaterial({ visible: false }));
    this.ghostQuad.frustumCulled = false;
    this.ghostQuad.renderOrder = 10;
    this.scene.add(this.quad, this.ghostQuad);

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
    const g = new THREE.Group();
    const minor = new THREE.GridHelper(400, 80, 0x000000, 0x000000);
    minor.rotation.x = Math.PI / 2;
    (minor.material as THREE.Material).transparent = true;
    (minor.material as THREE.Material).opacity = 0.06;
    const major = new THREE.GridHelper(400, 8, 0x000000, 0x000000);
    major.rotation.x = Math.PI / 2;
    (major.material as THREE.Material).transparent = true;
    (major.material as THREE.Material).opacity = 0.12;
    const axes = new THREE.Group();
    const axis = (dir: Vec3, color: number) => {
      const geo = new THREE.BufferGeometry().setFromPoints([new THREE.Vector3(0, 0, 0), new THREE.Vector3(...dir).multiplyScalar(20)]);
      const line = new THREE.Line(geo, new THREE.LineBasicMaterial({ color, transparent: true, opacity: 0.7 }));
      axes.add(line);
    };
    axis([1, 0, 0], 0xd0453b);
    axis([0, 1, 0], 0x3f9b4a);
    axis([0, 0, 1], 0x3b6fd0);
    g.add(minor, major, axes);
    return g;
  }

  setGridColor(dark: boolean) {
    const c = dark ? 0xffffff : 0x000000;
    this.grid.traverse((o) => {
      if (o instanceof THREE.GridHelper) {
        const m = o.material as THREE.LineBasicMaterial;
        m.color.setHex(c);
        m.vertexColors = false;
        m.needsUpdate = true;
      }
    });
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

  private buildMaterial(spec: FieldSpec, ghost: boolean): THREE.RawShaderMaterial {
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
      u_hlKey: { value: Array.from({ length: MAX_HIGHLIGHTS }, () => new THREE.Vector2()) },
      u_hlCol: { value: Array.from({ length: MAX_HIGHLIGHTS }, () => new THREE.Vector3()) },
      u_hlN: { value: 0 },
    };
    if (spec.grid && spec.source.includes('u_param')) {
      this.paramTexture?.dispose();
      const g = spec.grid;
      const tex = new THREE.Data3DTexture(g.data, g.dims[0], g.dims[1], g.dims[2]);
      tex.format = THREE.RedFormat;
      tex.type = THREE.FloatType;
      tex.minFilter = THREE.NearestFilter;
      tex.magFilter = THREE.NearestFilter;
      tex.unpackAlignment = 1;
      tex.needsUpdate = true;
      this.paramTexture = tex;
      uniforms.u_param = { value: tex };
    }
    return new THREE.RawShaderMaterial({
      glslVersion: THREE.GLSL3,
      vertexShader: VERT,
      fragmentShader: fragSource(spec.source, ghost ? NO_IDENT : spec.ident || NO_IDENT),
      uniforms,
      depthTest: true,
      depthWrite: !ghost,
      transparent: ghost,
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
    if (!spec.bounds) {
      this.material?.dispose();
      this.material = null;
      this.bounds = null;
      this.quad.material = new THREE.MeshBasicMaterial({ visible: false });
      this.invalidate();
      return;
    }
    const mat = this.buildMaterial(spec, false);
    const probe = new THREE.Mesh(this.quad.geometry, mat);
    probe.frustumCulled = false;
    try {
      await this.renderer.compileAsync(probe, this.camera, this.scene);
    } catch {
      /* fall back to synchronous compile on first render */
    }
    if (gen !== this.fieldGen) {
      mat.dispose();
      return;
    }
    this.material?.dispose();
    this.material = mat;
    this.bounds = spec.bounds;
    this.quad.material = mat;
    this.applyHighlights();
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

  /** Base surface colour (linear RGB). */
  setBaseColor(c: [number, number, number]) {
    this.baseColor = c;
    if (this.material) (this.material.uniforms.u_base.value as THREE.Color).setRGB(...c);
    this.invalidate();
  }
  private baseColor: [number, number, number] = [0.74, 0.73, 0.7];

  /** Translucent ghost of a shape (e.g. a selected cutter); `null` hides it. */
  setGhost(spec: FieldSpec | null) {
    this.ghostMaterial?.dispose();
    if (!spec || !spec.bounds) {
      this.ghostMaterial = null;
      this.ghostQuad.material = new THREE.MeshBasicMaterial({ visible: false });
    } else {
      this.ghostMaterial = this.buildMaterial(spec, true);
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
    if (this.ghostMaterial) this.updateUniforms(this.ghostMaterial, this.ghostMaterial.userData.bounds);
    this.quad.visible = this.showField;
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
