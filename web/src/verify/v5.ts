// V5 — the preview shader and the Rust evaluator compute the same field.
//
// 10 000 seeded random points in (and around) the bracket's bounds are evaluated twice: by the
// generated GLSL running on the GPU (rendered into an RGBA32F target, one pixel per point) and by
// the Rust evaluator via WebAssembly (the code path used for meshing, volumes and picking). Both
// sides receive identical f32-rounded coordinates.
import { loadWasm, parseFsm, bodyShader, evalBody } from '../doc/wasm';
import bracketFsm from '../../../examples/bracket.fsm?raw';

export interface V5Result {
  points: number;
  maxAbs: number;
  maxAbsNear: number;
  rms: number;
  maxGradAngleDeg: number;
  worst: { p: number[]; gpu: number; rust: number };
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

export async function runV5(): Promise<V5Result> {
  await loadWasm();
  const r = parseFsm(bracketFsm);
  if ('error' in r) throw new Error(`bracket.fsm: ${r.error.message}`);
  const doc = r.ok;
  const sh = bodyShader(doc);
  if (!sh.bounds) throw new Error('empty bracket');
  const W = 100;
  const H = 100;
  const N = W * H;
  const rand = mulberry32(5);
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
void main() { vec3 p = texelFetch(u_pts, ivec2(gl_FragCoord.xy), 0).xyz; o = fs_field(p); }`;
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
  gl.useProgram(prog);

  const tex = gl.createTexture();
  gl.activeTexture(gl.TEXTURE0);
  gl.bindTexture(gl.TEXTURE_2D, tex);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.NEAREST);
  gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.NEAREST);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA32F, W, H, 0, gl.RGBA, gl.FLOAT, pts32);
  gl.uniform1i(gl.getUniformLocation(prog, 'u_pts'), 0);

  const target = gl.createTexture();
  gl.bindTexture(gl.TEXTURE_2D, target);
  gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA32F, W, H, 0, gl.RGBA, gl.FLOAT, null);
  const fb = gl.createFramebuffer();
  gl.bindFramebuffer(gl.FRAMEBUFFER, fb);
  gl.framebufferTexture2D(gl.FRAMEBUFFER, gl.COLOR_ATTACHMENT0, gl.TEXTURE_2D, target, 0);
  gl.viewport(0, 0, W, H);

  const buf = gl.createBuffer();
  gl.bindBuffer(gl.ARRAY_BUFFER, buf);
  gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 3, -1, -1, 3]), gl.STATIC_DRAW);
  const loc = gl.getAttribLocation(prog, 'pos');
  gl.enableVertexAttribArray(loc);
  gl.vertexAttribPointer(loc, 2, gl.FLOAT, false, 0, 0);
  gl.activeTexture(gl.TEXTURE0);
  gl.bindTexture(gl.TEXTURE_2D, tex);
  gl.drawArrays(gl.TRIANGLES, 0, 3);
  const out = new Float32Array(N * 4);
  gl.readPixels(0, 0, W, H, gl.RGBA, gl.FLOAT, out);

  const rust = evalBody(doc, pts64);
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
    renderer,
    spacingNote: `points uniform in the bounds expanded by 5 mm: [${lo.map((v) => v.toFixed(1)).join(', ')}] – [${hi.map((v) => v.toFixed(1)).join(', ')}] mm`,
  };
}
