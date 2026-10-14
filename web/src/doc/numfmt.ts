// Rust f64 Display in the browser. The sweep's numbers cross the native/wasm boundary as
// text — error messages, CSV cells — so both sides must print the same digits. Rust's
// `f64::to_string` and JS's shortest round-trip printer agree inside [1e-6, 1e21): plain
// decimals, same digits. Outside that window JS switches to exponent form while Rust keeps
// printing the digits, so this printer walks the exponent back into the plain form. That
// same range note lives in crates/fieldsmith-api/src/sweep_csv.rs; the lens e2e checks the
// actual columns rather than assuming the window.

export function rustFloat(v: number): string {
  if (Number.isNaN(v)) return 'NaN';
  if (v === Infinity) return 'inf';
  if (v === -Infinity) return '-inf';
  if (Object.is(v, -0)) return '-0';
  const s = String(v);
  const m = /^(-?)(\d+)(?:\.(\d+))?e([+-]?\d+)$/.exec(s);
  if (!m) return s;
  const neg = m[1];
  const digits = (m[2] + (m[3] ?? '')).replace(/0+$/, '') || '0';
  const point = m[2].length + Number(m[4]);
  if (point <= 0) return `${neg}0.${'0'.repeat(-point)}${digits}`;
  if (point >= digits.length) return `${neg}${digits}${'0'.repeat(point - digits.length)}`;
  return `${neg}${digits.slice(0, point)}.${digits.slice(point)}`;
}