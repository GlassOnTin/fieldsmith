import { render } from 'preact';
import './styles.css';
import { App } from './app/App';
import { loadWasm } from './doc/wasm';
import { Store, store } from './doc/store';
import { installShortcuts, viewApi } from './app/commands';
import { ui } from './app/ui';

async function main() {
  await loadWasm();
  const saved = Store.restore();
  if (saved) store.load(saved);
  installShortcuts();
  // Expose internals for automation (headless tests drive the UI and read results through it).
  (window as unknown as { fieldsmith: Record<string, unknown> }).fieldsmith = { store, ui, view: viewApi };
  render(<App />, document.getElementById('app')!);
}

main().catch((e) => {
  document.body.textContent = `Fieldsmith failed to start: ${e instanceof Error ? e.message : String(e)}`;
  console.error(e);
});
