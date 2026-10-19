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
  // With no fill there is nothing behind the body to show; the slider would only fade
  // the part into the background (reads as "dimmer", the V16 mechanic needs a core pass).
  // Say so instead of pretending: disabled slider, and the hint.
  const filled = !!store.doc?.fill;
  return (
    <div class="opacity-panel" data-testid="opacity-panel">
      <span class="plain">Semi-transparent</span>
      <input
        type="range"
        min={0}
        max={1}
        step={0.01}
        value={ui.bodyAlpha}
        title="Body opacity: 0 = only the fill shows"
        data-testid="opacity-slider"
        disabled={!filled}
        onInput={(e) => ui.set('bodyAlpha', Number.parseFloat((e.target as HTMLInputElement).value))}
      />
      <span class="num" data-testid="opacity-value" data-filled={filled ? '1' : '0'}>
        {filled ? `${Math.round(ui.bodyAlpha * 100)}%` : '—'}
      </span>
      {!filled && (
        <span class="meta" data-testid="opacity-hint">
          nothing to see through without a fill — add one in the Fill lens
        </span>
      )}
    </div>
  );
}
viewOverlays.push(OpacityPanel);