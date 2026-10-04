import { render } from 'preact';
import './styles.css';
import { App } from './app/App';
import { loadWasm, pick } from './doc/wasm';
import { Store, store } from './doc/store';
import { installShortcuts, viewApi } from './app/commands';
import { ui } from './app/ui';
import { analysis } from './app/analysis';
import { liveViewport } from './app/ViewportHost';
// Lenses register their overlays/inspectors/tree sections as side effects.
import './app/setup';
import './app/results';
import './app/fill';
import './app/make';

async function main() {
  await loadWasm();
  const saved = Store.restore();
  if (saved) store.load(saved);
  installShortcuts();
  // Expose internals for automation (headless tests drive the UI and read results through it).
  (window as unknown as { fieldsmith: Record<string, unknown> }).fieldsmith = {
    store,
    ui,
    analysis,
    view: viewApi,
    /** Ray from the live camera through a client pixel, into the document. */
    pickAt: (x: number, y: number) => {
      const vp = liveViewport;
      if (!vp) return null;
      const r = vp.ray(x, y);
      return pick(store.doc, r.origin, r.dir);
    },
  };
  render(<App />, document.getElementById('app')!);
}

main().catch((e) => {
  document.body.textContent = `Fieldsmith failed to start: ${e instanceof Error ? e.message : String(e)}`;
  console.error(e);
});