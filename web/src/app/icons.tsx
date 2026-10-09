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
};

export function ShapeIcon({ type }: { type: string }) {
  const f = Icon[type] ?? Icon.box;
  return f();
}
