// View opacity overlay: the "see-through fill" control. Drags `ui.bodyAlpha`, which the
// ViewportHost comparator forwards to the viewport — the body renders translucent and the
// interior (lattice) pass shows through. Session-only: resets to opaque on load, matching
// the scope decision that UI state is not persisted. Hidden in the results lens, which has
// no field shader to make transparent.
import { ui, useApp } from './ui';
import { store } from '../doc/store';
import { viewOverlays } from './App';

function OpacityPanel() {
  useApp();
  if (store.lens === 'results') return null;
  return (
    <div class="opacity-panel" data-testid="opacity-panel">
      <span class="plain">Semi-transparent</span>
      <input
        type="range"
        min={0}
        max={1}
        step={0.01}
        value={ui.bodyAlpha}
        title="Body opacity: seeing through shows the far wall (and the fill behind it)"
        data-testid="opacity-slider"
        onInput={(e) => ui.set('bodyAlpha', Number.parseFloat((e.target as HTMLInputElement).value))}
      />
      <span class="num" data-testid="opacity-value">
        {Math.round(ui.bodyAlpha * 100)}%
      </span>
    </div>
  );
}
viewOverlays.push(OpacityPanel);