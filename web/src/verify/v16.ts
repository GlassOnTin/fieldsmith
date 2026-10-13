// V16 — the interior (core) field: the viewport shader's `fs_core` against the Rust
// `removed_skin` evaluator.
//
// The filled bracket (`examples/bracket-filled.fsm`) is compiled to the viewport program,
// which carries the body field as `fs_field` and the fill with the outer skin removed as
// `fs_core`. 10 000 seeded random points around the bounds are evaluated twice: by `fs_core`
// running on the GPU (one pixel per point in an RGBA32F target) and by the Rust evaluator
// via WebAssembly. Each input (the points and the `u_param` grid) is first round-tripped
// through the GPU itself — a pass-through draw plus readback — and the Rust side receives
// those exact values: some stacks store float-texture data at half precision (measured on
// ANGLE/NVIDIA, fp16 quantum 0.0625 at |p| ≈ 65), and the comparison must bound codegen
// mistakes at the shader's real coordinates, not differences caused by the upload.
import { loadWasm, parseFsm, bodyShader, evalCoreWithParam, previewFillParamData } from '../doc/wasm';
import bracketFilledFsm from '../../../examples/bracket-filled.fsm?raw';

export interface V16Result {
  points: number;
  maxAbs: number;
  maxAbsNear: number;
  rms: number;
  maxGradAngleDeg: number;
  worst: { p: number[]; gpu: number; rust: number };
  grid: { dims: [number, number, number]; spacing: number };
  renderer: string;
  spacingNote: string;
}

function mulberry32(seed: number) {
  return () => {
    seed |= 0;
    seed = (seed + 0x6d2b79f5) | 0;
    let t = Math.imul(seed ^ (seed >>> 15), 1 | seed);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

export async function runV16(): Promise<V16Result> {
  await loadWasm();
  const r = parseFsm(bracketFilledFsm);
  if ('error' in r) throw new Error(`bracket-filled.fsm: ${r.error.message}`);
  const doc = r.ok;
  const sh = bodyShader(doc);
  if (!sh.bounds) throw new Error('empty document');
  if (!sh.core || !sh.grid) throw new Error('the body shader carries no interior field for a filled document');
  const [nx, ny, nz] = sh.grid.dims;
  const param = previewFillParamData(doc);
  if (param.length !== nx * ny * nz) throw new Error(`parameter grid is ${param.length} values, dims say ${nx}·${ny}·${nz}`);

  const W = 100;
  const H = 100;
  const N = W * H;
  const rand = mulberry32(16);
  const lo = sh.bounds[0].map((v) => v - 5);
  const hi = sh.bounds[1].map((v) => v + 5);
  const pts32 = new Float32Array(N * 4);
  const pts64 = new Float64Array(N * 3);
  for (let i = 0; i < N; i++) {
    for (let a = 0; a < 3; a++) {
      const v = Math.fround(lo[a] + (hi[a] - lo[a]) * rand());
      pts32[i * 4 + a] = v;
      pts64[i * 3 + a] = v;
    }
  }

  const canvas = document.createElement('canvas');
  canvas.width = W;
  canvas.height = H;
  const gl = canvas.getContext('webgl2');
  if (!gl) throw new Error('WebGL2 unavailable');
  if (!gl.getExtension('EXT_color_buffer_float')) throw new Error('EXT_color_buffer_float unavailable');
  const dbg = gl.getExtension('WEBGL_debug_renderer_info');
  const renderer = dbg ? String(gl.getParameter(dbg.UNMASKED_RENDERER_WEBGL)) : 'unknown';

  const vs = `#version 300 es
in vec2 pos; void main() { gl_Position = vec4(pos, 0.0, 1.0); }`;
  const fs = `#version 300 es
precision highp float; precision highp int; precision highp sampler3D;
uniform sampler2D u_pts;
out vec4 o;
${sh.source}
void main() { vec3 p = texelFetch(u_pts, ivec2(gl_FragCoord.xy), 0).xyz; o = fs_core(p); }`;
  const compile = (type: number, src: string) => {
    const s = gl.createShader(type)!;
    gl.shaderSource(s, src);
    gl.compileShader(s);
    if (!gl.getShaderParameter(s, gl.COMPILE_STATUS)) throw new Error(gl.getShaderInfoLog(s) ?? 'shader error');
    return s;
  };
  const prog = gl.createProgram()!;
  gl.attachShader(prog, compile(gl.VERTEX_SHADER, vs));
  gl.attachShader(prog, compile(gl.FRAGMENT_SHADER, fs));
  gl.linkProgram(prog);
  if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(prog) ?? 'link error');
  const pass = `#version 300 es
precision highp float; precision highp int;
uniform sampler2D u_pts;
out vec4 o;
void main() { o = texelFetch(u_pts, ivec2(gl_FragCoord.xy), 0); }`;
  const pprog = gl.createProgram()!;
  gl.attachShader(pprog, compile(gl.VERTEX_SHADER, vs));
  gl.attachShader(pprog, compile(gl.FRAGMENT_SHADER, pass));
  gl.linkProgram(pprog);
  if (!gl.getProgramParameter(pprog, gl.LINK_STATUS)) throw new Error(gl.getProgramInfoLog(pprog) ?? 'link error');

  // Output surfaces (the fs_core draw lands on the points one). Neither is bound as a
  // sampler while a program draws on it.
  const rtPts = gl.createTexture();
  gl.activeTexture(gl.TEXTURE6);
  gl.bindTexture(gl.TEXTURE_2D, rtPts);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA32F, W, H, 0, gl.RGBA, gl.FLOAT, null);
  gl.bindTexture(gl.TEXTURE_2D, null);
  const fb = gl.createFramebuffer();
  gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
  const tri = new Float32Array([-1, -1, 3, -1, -1, 3]);
  const setupAttribs = (p: WebGLProgram) => {
    const verts = gl.createBuffer();
    gl.bindBuffer(gl.ARRAY_BUFFER, verts);
    gl.bufferData(gl.ARRAY_BUFFER, tri, gl.STATIC_DRAW);
    const loc = gl.getAttribLocation(p, 'pos');
    gl.enableVertexAttribArray(loc);
    gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
    return verts;
  };

  /** Round trip 1 — the points: a pass-through draw over the very texture the fs_core
   * program will read; the readback's x/y/z are the stored texel values the Rust side must
   * be evaluated at. Returns nothing directly; fills pts64. */
  gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, rtPts, 0);
  gl.viewport(0, 0, W, H);
  const ptsTex = gl.createTexture();
  gl.activeTexture(gl.TEXTURE3);
  gl.bindTexture(gl.TEXTURE_2D, ptsTex);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA32F, W, H, 0, gl.RGBA, gl.FLOAT, pts32);
  gl.activeTexture(gl.TEXTURE3);
  gl.bindTexture(gl.TEXTURE_2D, ptsTex);
  gl.useProgram(pprog);
  gl.uniform1i(gl.getUniformLocation(pprog, 'u_pts'), 3);
  const vertsP = setupAttribs(pprog);
  gl.drawArrays(gl.TRIANGLES, 0, 3);
  const ptsBack = new Float32Array(N * 4);
  gl.readPixels(0, 0, W, H, gl.RGBA, gl.FLOAT, ptsBack);
  for (let i = 0; i < N; i++) {
    pts64[i * 3] = ptsBack[i * 4];
    pts64[i * 3 + 1] = ptsBack[i * 4 + 1];
    pts64[i * 3 + 2] = ptsBack[i * 4 + 2];
  }

  // Round trip 2 — the parameter grid: flattened into a 2D RGBA32F texture (nx × ny·nz,
  // slice-major = the 3D texture's own texel order), read back, then uploaded to the R32F
  // 3D texture the fs program interpolates. fp16 is idempotent, so re-uploading the
  // read-back values is exact on every stack.
  gl.pixelStorei(gl.UNPACK_ALIGNMENT, 1);
  const paramTex = gl.createTexture();
  gl.activeTexture(gl.TEXTURE4);
  gl.bindTexture(gl.TEXTURE_2D, paramTex);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
  const paramSrc = new Float32Array(param.length * 4);
  for (let i = 0; i < param.length; i++) paramSrc[i * 4] = param[i];
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA32F, nx, ny * nz, 0, gl.RGBA, gl.FLOAT, paramSrc);
  const rtParam = gl.createTexture();
  gl.activeTexture(gl.TEXTURE6);
  gl.bindTexture(gl.TEXTURE_2D, rtParam);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA32F, nx, ny * nz, 0, gl.RGBA, gl.FLOAT, null);
  gl.activeTexture(gl.TEXTURE6);
  gl.bindTexture(gl.TEXTURE_2D, null);
  gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, rtParam, 0);
  gl.activeTexture(gl.TEXTURE3);
  gl.bindTexture(gl.TEXTURE_2D, paramTex);
  const vertsQ = setupAttribs(pprog);
  gl.drawArrays(gl.TRIANGLES, 0, 3);
  const paramBack = new Float32Array(param.length * 4);
  gl.readPixels(0, 0, nx, ny * nz, gl.RGBA, gl.FLOAT, paramBack);
  const paramRT = paramBack.filter((_, i) => i % 4 === 0);
  const ptex = gl.createTexture();
  gl.activeTexture(gl.TEXTURE5);
  gl.bindTexture(gl.TEXTURE_3D, ptex);
  gl.texParameteri(gl.TEXTURE_3D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
  gl.texParameteri(gl.TEXTURE_3D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
  gl.texParameteri(gl.TEXTURE_3D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_3D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
  gl.texParameteri(gl.TEXTURE_3D, gl.TEXTURE_WRAP_R, gl.CLAMP_TO_EDGE);
  gl.texImage3D(gl.TEXTURE_3D, 0, gl.R32F, nx, ny, nz, 0, gl.RED, gl.FLOAT, paramRT);

  // The measured pass: fs_core over the round-tripped points texture.
  gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, rtPts, 0);
  gl.viewport(0, 0, W, H);
  gl.activeTexture(gl.TEXTURE3);
  gl.bindTexture(gl.TEXTURE_2D, ptsTex);
  gl.useProgram(prog);
  gl.uniform1i(gl.getUniformLocation(prog, 'u_pts'), 3);
  gl.uniform1i(gl.getUniformLocation(prog, 'u_param'), 5);
  const vertsR = setupAttribs(prog);
  gl.drawArrays(gl.TRIANGLES, 0, 3);
  const out = new Float32Array(N * 4);
  gl.readPixels(0, 0, W, H, gl.RGBA, gl.FLOAT, out);
  gl.deleteBuffer(vertsP);
  gl.deleteBuffer(vertsQ);
  gl.deleteBuffer(vertsR);

  const rust = evalCoreWithParam(doc, paramRT, pts64);
  let maxAbs = 0;
  let maxAbsNear = 0;
  let sum2 = 0;
  let maxAng = 0;
  let worst = { p: [0, 0, 0], gpu: 0, rust: 0 };
  for (let i = 0; i < N; i++) {
    const g = out[i * 4];
    const rv = rust[i * 4];
    const e = Math.abs(g - rv);
    sum2 += e * e;
    if (e > maxAbs) {
      maxAbs = e;
      worst = { p: [pts64[i * 3], pts64[i * 3 + 1], pts64[i * 3 + 2]], gpu: g, rust: rv };
    }
    if (Math.abs(rv) < 2 && e > maxAbsNear) maxAbsNear = e;
    const ga = [out[i * 4 + 1], out[i * 4 + 2], out[i * 4 + 3]];
    const gb = [rust[i * 4 + 1], rust[i * 4 + 2], rust[i * 4 + 3]];
    const la = Math.hypot(...ga);
    const lb = Math.hypot(...gb);
    if (la > 1e-6 && lb > 1e-6) {
      const c = Math.min(1, Math.max(-1, (ga[0] * gb[0] + ga[1] * gb[1] + ga[2] * gb[2]) / (la * lb)));
      maxAng = Math.max(maxAng, (Math.acos(c) * 180) / Math.PI);
    }
  }
  return {
    points: N,
    maxAbs,
    maxAbsNear,
    rms: Math.sqrt(sum2 / N),
    maxGradAngleDeg: maxAng,
    worst,
    grid: { dims: [nx, ny, nz], spacing: sh.grid.spacing },
    renderer,
    spacingNote: `points uniform in the bounds expanded by 5 mm: [${lo.map((v) => v.toFixed(1)).join(', ')}] – [${hi.map((v) => v.toFixed(1)).join(', ')}] mm`,
  };
}