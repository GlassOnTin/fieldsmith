// Section-plane overlay: the "slice through the part" control. Toggling on picks the
// camera-relative direction automatically (the half nearest the camera is cut away) and
// the slider sweeps the plane through the field bounds; flip + slider moves to the
// complementary half at the same depth (flip pairs t → 1−t). ViewportHost forwards
// `ui.clip` to the viewport, so both the raymarched body and the results mesh ride the
// same plane. Session-only, like body opacity.
import { ui, useApp } from './ui';
import { store } from '../doc/store';
import { liveViewport } from './ViewportHost';
import { viewOverlays } from './App';

function ClipPanel() {
  useApp();
  if (!store.doc) return null;
  const clip = ui.clip;
  const toggleClip = () => {
    const vp = liveViewport;
    const b = vp?.bounds;
    if (!clip.on) {
      // Auto-direction: cut the half nearest the camera so the section opens toward the
      // viewer. The cap normal (−n) then faces the camera and lights properly. The plane
      // lands mid-part: t = 0 is the no-cut rest state by design (the driver routes it
      // inert for a bit-identical rest frame), so engaging the section must move off it
      // or the toggle would read as a dead button.
      let dir: 1 | -1 = 1;
      if (b && vp) {
        const cam = vp.camera.position.toArray();
        const mid = (b[0][clip.axis] + b[1][clip.axis]) / 2;
        dir = cam[clip.axis] > mid ? -1 : 1;
      }
      ui.set('clip', { ...clip, on: true, dir, t: 0.5 });
    } else {
      ui.set('clip', { ...clip, on: false, t: 0 });
    }
  };
  const flip = () => {
    // Plane position must not move: dir=+1 sits at lo + t·span, dir=−1 at hi − t'·span;
    // equal when t' = 1 − t.
    ui.set('clip', { axis: clip.axis, dir: -clip.dir as 1 | -1, on: clip.on, t: 1 - clip.t });
  };
  const setAxis = (axis: 0 | 1 | 2) => () => {
    ui.set('clip', { ...clip, axis });
  };
  return (
    <div class="clip-panel" data-testid="clip-panel">
      <button
        class={'btn ghost' + (clip.on ? ' on' : '')}
        data-testid="clip-toggle"
        title="Section plane: cut away the material on the camera side of the plane"
        onClick={toggleClip}
      >
        Section
      </button>
      <button
        class={'btn ghost' + (clip.axis === 0 ? ' on' : '')}
        data-testid="clip-axis-x"
        title="Slide the plane along x"
        onClick={setAxis(0)}
      >
        X
      </button>
      <button
        class={'btn ghost' + (clip.axis === 1 ? ' on' : '')}
        data-testid="clip-axis-y"
        title="Slide the plane along y"
        onClick={setAxis(1)}
      >
        Y
      </button>
      <button
        class={'btn ghost' + (clip.axis === 2 ? ' on' : '')}
        data-testid="clip-axis-z"
        title="Slide the plane along z"
        onClick={setAxis(2)}
      >
        Z
      </button>
      <button
        class="btn ghost"
        data-testid="clip-flip"
        title="Flip the kept half (plane stays put)"
        onClick={flip}
      >
        Flip
      </button>
      <input
        type="range"
        min={0}
        max={1}
        step={0.005}
        value={clip.t}
        title="Section depth along the axis"
        data-testid="clip-slider"
        onInput={(e) => ui.set('clip', { ...ui.clip, t: Number.parseFloat((e.target as HTMLInputElement).value) })}
      />
      <span class="num" data-testid="clip-value">
        {Math.round(clip.t * 100)}%
      </span>
    </div>
  );
}
viewOverlays.push(ClipPanel);