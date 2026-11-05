// Small line icons (16 px grid, currentColor). No emoji anywhere in the UI.
import type { JSX } from 'preact';

const S = (props: { children: JSX.Element | JSX.Element[] }) => (
  <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {props.children}
  </svg>
);

export const Icon: Record<string, () => JSX.Element> = {
  box: () => (
    <S>
      <path d="M8 1.8 13.5 4.8v6.4L8 14.2 2.5 11.2V4.8z" />
      <path d="M2.5 4.8 8 7.8l5.5-3M8 7.8v6.4" />
    </S>
  ),
  cylinder: () => (
    <S>
      <ellipse cx="8" cy="4" rx="5" ry="2" />
      <path d="M3 4v8c0 1.1 2.2 2 5 2s5-.9 5-2V4" />
    </S>
  ),
  sphere: () => (
    <S>
      <circle cx="8" cy="8" r="5.8" />
      <ellipse cx="8" cy="8" rx="5.8" ry="2.2" />
    </S>
  ),
  capsule: () => (
    <S>
      <rect x="4.5" y="1.8" width="7" height="12.4" rx="3.5" />
    </S>
  ),
  torus: () => (
    <S>
      <ellipse cx="8" cy="8" rx="6" ry="3.6" />
      <ellipse cx="8" cy="7.6" rx="2.4" ry="1.2" />
    </S>
  ),
  extrude: () => (
    <S>
      <path d="M2.5 11.5h7l3-3v-6h-7l-3 3z" />
      <path d="M2.5 5.5h7v6M9.5 5.5l3-3" />
    </S>
  ),
  revolve: () => (
    <S>
      <path d="M8 2v12" stroke-dasharray="1.5 1.5" />
      <path d="M9.5 3.5h3v9h-3z" />
      <path d="M6.5 4.5a4.5 2 0 1 0 3 0" />
    </S>
  ),
  group: () => (
    <S>
      <path d="M2 4.5h4.5l1.2 1.5H14v7.5H2z" />
    </S>
  ),
  plus: () => (
    <S>
      <path d="M8 3v10M3 8h10" />
    </S>
  ),
  eye: () => (
    <S>
      <path d="M1.5 8s2.4-4.5 6.5-4.5S14.5 8 14.5 8 12.1 12.5 8 12.5 1.5 8 1.5 8z" />
      <circle cx="8" cy="8" r="2" />
    </S>
  ),
  eyeOff: () => (
    <S>
      <path d="M2.5 2.5l11 11M6.6 3.7A6.3 6.3 0 0 1 8 3.5c4.1 0 6.5 4.5 6.5 4.5a12 12 0 0 1-1.9 2.4M10.2 12a6 6 0 0 1-2.2.5C3.9 12.5 1.5 8 1.5 8a12.4 12.4 0 0 1 2.4-2.9" />
    </S>
  ),
  trash: () => (
    <S>
      <path d="M2.5 4h11M6 4V2.5h4V4M4 4l.7 9.5h6.6L12 4" />
    </S>
  ),
  edit: () => (
    <S>
      <path d="M11.2 2.3 13.7 4.8 5.4 13.1 2.2 13.8 2.9 10.6z" />
      <path d="M9.6 3.9 12.1 6.4" />
    </S>
  ),
  chevron: () => (
    <S>
      <path d="M5.5 3.5 10 8l-4.5 4.5" />
    </S>
  ),
  folder: () => (
    <S>
      <path d="M2 4.5h4l1.2 1.5H14v7H2z" />
      <path d="M2 4.5v-1h4l1 1" />
    </S>
  ),
  mesh: () => (
    <S>
      <path d="M8 2 13.5 5v6L8 14 2.5 11V5z" />
      <path d="M2.5 5 8 8l5.5-3M8 8v6M5.25 3.5l5.5 3M10.75 3.5l-5.5 3" />
    </S>
  ),
  menu: () => (
    <S>
      <path d="M2.5 4.5h11M2.5 8h11M2.5 11.5h11" />
    </S>
  ),
  fit: () => (
    <S>
      <path d="M2 5.5V2h3.5M10.5 2H14v3.5M14 10.5V14h-3.5M5.5 14H2v-3.5" />
    </S>
  ),
  undo: () => (
    <S>
      <path d="M5.5 3.5 2.5 6.5l3 3" />
      <path d="M2.5 6.5H10a3.5 3.5 0 0 1 0 7H7" />
    </S>
  ),
  redo: () => (
    <S>
      <path d="m10.5 3.5 3 3-3 3" />
      <path d="M13.5 6.5H6a3.5 3.5 0 0 0 0 7h3" />
    </S>
  ),
  move: () => (
    <S>
      <path d="M8 1.5v13M1.5 8h13M8 1.5 6.3 3.2M8 1.5l1.7 1.7M8 14.5l-1.7-1.7M8 14.5l1.7-1.7M1.5 8l1.7-1.7M1.5 8l1.7 1.7M14.5 8l-1.7-1.7M14.5 8l-1.7 1.7" />
    </S>
  ),
  rotate: () => (
    <S>
      <path d="M13 8a5 5 0 1 1-1.5-3.6" />
      <path d="M13 2.5v2.5h-2.5" />
    </S>
  ),
  magnet: () => (
    <S>
      <path d="M3.5 2.5v5a4.5 4.5 0 0 0 9 0v-5h-3v5a1.5 1.5 0 0 1-3 0v-5z" />
      <path d="M3.5 5h3M9.5 5h3" />
    </S>
  ),
  sliders: () => (
    <S>
      <path d="M2.5 5h11M2.5 11h11" />
      <path d="M10 3v4M6 9v4" />
    </S>
  ),
  region: () => (
    <S>
      <path d="M2.5 11.5 8 14l5.5-2.5V4.5L8 2 2.5 4.5z" stroke-dasharray="2 1.6" />
      <path d="M5 7.5 8 9l3-1.5" />
    </S>
  ),
  support: () => (
    <S>
      <path d="M8 2.5v6M4 8.5h8l-4 0M4 12.5h8M5 8.5l-1.5 4M8 8.5v4M11 8.5l1.5 4" />
    </S>
  ),
  load: () => (
    <S>
      <path d="M8 1.5v10M4.5 8 8 11.5 11.5 8M3 14.5h10" />
    </S>
  ),
  material: () => (
    <S>
      <path d="M8 1.8 13.5 5v6L8 14.2 2.5 11V5z" />
      <path d="M2.5 5 8 8.2 13.5 5M8 8.2v6" stroke-dasharray="1.5 1.5" />
    </S>
  ),
  sidebarLeft: () => (
    <S>
      <rect x="2" y="2.5" width="12" height="11" rx="1.5" />
      <path d="M6 2.5v11" />
    </S>
  ),
  sidebarRight: () => (
    <S>
      <rect x="2" y="2.5" width="12" height="11" rx="1.5" />
      <path d="M10 2.5v11" />
    </S>
  ),
  sidebarsBoth: () => (
    <S>
      <rect x="2" y="2.5" width="12" height="11" rx="1.5" />
      <path d="M5.5 2.5v11M10.5 2.5v11" />
    </S>
  ),
  // ---- Parametric Sketcher & Constraint Solver Icons (FreeCAD / SolveSpace / Shapr3D visual language) ----
  sketcher: () => (
    <S>
      <rect x="2" y="2" width="12" height="12" rx="1.5" stroke-dasharray="2 1.5" />
      <path d="M4.5 11.5h7v-5l-3.5-2.5-3.5 2.5z" />
      <circle cx="4.5" cy="11.5" r="1.2" fill="currentColor" />
      <circle cx="11.5" cy="11.5" r="1.2" fill="currentColor" />
    </S>
  ),
  fixing: () => (
    <S>
      <path d="M4.5 2.5h7v2.5h-7z" />
      <path d="M6.2 5v8.5l1.8 1.2 1.8-1.2V5" />
      <path d="M6.2 7.2h3.6M6.2 9.5h3.6M6.2 11.8h3.6" />
    </S>
  ),
  // 2D Geometry Creation Icons
  geomLine: () => (
    <S>
      <path d="M3 13 13 3" />
      <circle cx="3" cy="13" r="1.6" fill="currentColor" />
      <circle cx="13" cy="3" r="1.6" fill="currentColor" />
    </S>
  ),
  geomRect: () => (
    <S>
      <rect x="2.5" y="3.5" width="11" height="9" />
      <circle cx="2.5" cy="3.5" r="1.4" fill="currentColor" />
      <circle cx="13.5" cy="12.5" r="1.4" fill="currentColor" />
      <circle cx="8" cy="8" r="1.1" fill="currentColor" />
    </S>
  ),
  geomCircle: () => (
    <S>
      <circle cx="8" cy="8" r="5.5" />
      <circle cx="8" cy="8" r="1.3" fill="currentColor" />
    </S>
  ),
  geomSlot: () => (
    <S>
      <rect x="2" y="5" width="12" height="6" rx="3" />
      <path d="M5 8h6" stroke-dasharray="1.5 1.5" />
    </S>
  ),
  geomBoltCircle: () => (
    <S>
      <circle cx="8" cy="8" r="5.2" stroke-dasharray="1.8 1.8" />
      <circle cx="8" cy="2.8" r="1.4" />
      <circle cx="13.2" cy="8" r="1.4" />
      <circle cx="8" cy="13.2" r="1.4" />
      <circle cx="2.8" cy="8" r="1.4" />
    </S>
  ),
  geomPolygon: () => (
    <S>
      <path d="M8 2.2 13 5.1v5.8L8 13.8 3 10.9V5.1z" />
      <circle cx="8" cy="8" r="1.8" />
    </S>
  ),
  geomFillet: () => (
    <S>
      <path d="M3 13V8a5 5 0 0 1 5-5h5" />
      <path d="M3 6.5V3h3.5" stroke-dasharray="1.3 1.3" />
      <circle cx="8" cy="8" r="1.2" fill="currentColor" />
    </S>
  ),
  // Geometric & Dimensional Constraint Icons (FreeCAD crimson/amber icon style)
  conCoincident: () => (
    <S>
      <circle cx="8" cy="8" r="4.2" />
      <circle cx="8" cy="8" r="1.8" fill="currentColor" />
      <path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2" />
    </S>
  ),
  conFixed: () => (
    <S>
      <rect x="4" y="7.5" width="8" height="6" rx="1" />
      <path d="M5.5 7.5V5.2a2.5 2.5 0 0 1 5 0v2.3" />
    </S>
  ),
  conHorizontal: () => (
    <S>
      <path d="M2 8h12" stroke-width="2" />
      <circle cx="2.5" cy="8" r="1.5" fill="currentColor" />
      <circle cx="13.5" cy="8" r="1.5" fill="currentColor" />
    </S>
  ),
  conVertical: () => (
    <S>
      <path d="M8 2v12" stroke-width="2" />
      <circle cx="8" cy="2.5" r="1.5" fill="currentColor" />
      <circle cx="8" cy="13.5" r="1.5" fill="currentColor" />
    </S>
  ),
  conParallel: () => (
    <S>
      <path d="M3 12 9 3M7 13l6-9" stroke-width="1.7" />
    </S>
  ),
  conPerpendicular: () => (
    <S>
      <path d="M3 13h10M8 13V3" stroke-width="1.7" />
      <path d="M8 10h3v3" />
    </S>
  ),
  conEqual: () => (
    <S>
      <path d="M3.5 6h9M3.5 10h9" stroke-width="1.9" />
    </S>
  ),
  conTangent: () => (
    <S>
      <circle cx="6.5" cy="9.5" r="4.2" />
      <path d="M2.5 13.5 13.5 2.5" stroke-width="1.6" />
    </S>
  ),
  conConcentric: () => (
    <S>
      <circle cx="8" cy="8" r="5.8" />
      <circle cx="8" cy="8" r="2.6" />
      <circle cx="8" cy="8" r="0.9" fill="currentColor" />
    </S>
  ),
  conSymmetric: () => (
    <S>
      <path d="M8 2v12" stroke-dasharray="1.6 1.6" />
      <path d="M5.5 5 2.5 8l3 3M10.5 5l3 3-3 3" />
    </S>
  ),
  conMidpoint: () => (
    <S>
      <path d="M2.5 13.5 13.5 2.5" />
      <circle cx="8" cy="8" r="2" fill="currentColor" />
    </S>
  ),
  conDistance: () => (
    <S>
      <path d="M2.5 4v8M13.5 4v8M2.5 8h11" />
      <path d="M4.8 6 2.5 8l2.3 2M11.2 6l2.3 2-2.3 2" />
    </S>
  ),
  conDiameter: () => (
    <S>
      <circle cx="8" cy="8" r="5" />
      <path d="M3.2 12.8 12.8 3.2" stroke-width="1.6" />
    </S>
  ),
  conAngle: () => (
    <S>
      <path d="M3 13h10M3 13 11 3.5" stroke-width="1.5" />
      <path d="M9.5 13a6.5 6.5 0 0 0-2.3-5" />
    </S>
  ),
  // Stage 2 3D Surface-Feature Operation Icons (Pad/Boss, Pocket/Cut, Revolve Boss, Groove Cut)
  featBossExtrude: () => (
    <S>
      <path d="M3 11.5 8 14l5-2.5V6L8 3.5 3 6z" />
      <path d="M3 6 8 8.5 13 6M8 8.5V14" />
      <path d="M8 1.2v3.5M6.3 2.8 8 1.2l1.7 1.6" stroke-width="1.6" />
    </S>
  ),
  featPocketCut: () => (
    <S>
      <path d="M2.5 11.5 8 14l5.5-2.5V5L8 2.5 2.5 5z" />
      <path d="M5.2 6.2 8 7.6l2.8-1.4v3.6L8 11.2 5.2 9.8z" stroke-dasharray="1.5 1.2" />
    </S>
  ),
  featRevolveBoss: () => (
    <S>
      <path d="M8 1.5v13" stroke-dasharray="1.5 1.5" />
      <path d="M9.8 3.5h3.2v9H9.8z" />
      <path d="M5.8 5a4.5 2.2 0 1 0 4 0" />
      <path d="M4.5 3.6 5.8 5l-1.8.8" />
    </S>
  ),
  featRevolveCut: () => (
    <S>
      <path d="M8 1.5v13" stroke-dasharray="1.5 1.5" />
      <path d="M9.8 3h3.4v3.2h-1.8v3.6h1.8V13H9.8z" />
      <path d="M5.8 5a4.5 2.2 0 1 0 4 0" stroke-dasharray="1.4 1.2" />
    </S>
  ),
};

export function ShapeIcon({ type }: { type: string }) {
  const f = Icon[type] ?? Icon.box;
  return f();
}
