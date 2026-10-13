// Browser verification entry: runs the cases and publishes results on `window.__verify`.
import { runV5 } from './v5';
import { runV16 } from './v16';

const out = document.getElementById('out')!;
(async () => {
  const results: Record<string, unknown> = {};
  try {
    results.V5 = await runV5();
  } catch (e) {
    results.V5 = { error: e instanceof Error ? e.message : String(e) };
  }
  try {
    results.V16 = await runV16();
  } catch (e) {
    results.V16 = { error: e instanceof Error ? e.message : String(e) };
  }
  (window as unknown as { __verify: unknown }).__verify = results;
  out.textContent = JSON.stringify(results, null, 2);
})();
