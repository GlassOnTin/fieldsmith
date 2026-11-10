// Lightweight async-chunk launchers for optional workbenches and reference-calculator modals.
// Importing from this module adds zero heavy workbench code to the initial 6-lens CAD/FEA bundle.

import type { Face } from '../doc/types';
import type { FixingKindId } from '../doc/fixings-catalog';
import { ui } from './ui';

export function openSketchWorkbenchModal(): void {
  ui.set('sketchOpen', true);
  void import('./SketchWorkbench').then((m) => m.openSketchWorkbenchModal());
}

export function openFixingsCatalogueModal(
  initialKind?: FixingKindId,
  targetHostId?: string,
  targetFace?: Face,
  initialTab?: 'catalogue' | 'workbench',
): void {
  ui.set('fixingsOpen', true);
  void import('./FixingsModal').then((m) =>
    m.openFixingsCatalogueModal(initialKind, targetHostId, targetFace, initialTab),
  );
}

export function openAntennaWorkbenchModal(
  presetMode: 'from_doc' | 'optimised' | 'unoptimised' = 'from_doc',
): void {
  ui.set('antennaOpen', true);
  void import('./AntennaWorkbench').then((m) => m.openAntennaWorkbenchModal(presetMode));
}

export function openFreeSurfaceModal(initialPreset?: string): void {
  ui.set('freeSurfaceOpen', true);
  void import('./FreeSurfaceModal').then((m) => m.openFreeSurfaceModal(initialPreset));
}

export function openCoordMappingModal(patch?: Record<string, unknown>): void {
  ui.set('coordMappingOpen', true);
  void import('./CoordMappingModal').then((m) =>
    m.openCoordMappingModal(patch as Parameters<typeof m.openCoordMappingModal>[0]),
  );
}

export function openChannelProfileModal(): void {
  ui.set('channelProfileOpen', true);
}

export function toggleTutorialsDrawer(): void {
  if (ui.tutorialsOpen && ui.tutorialMinimised) {
    ui.set('tutorialMinimised', false);
    return;
  }
  const nextOpen = !ui.tutorialsOpen;
  ui.set('tutorialMinimised', false);
  ui.set('tutorialsOpen', nextOpen);
  if (nextOpen) {
    void import('./tutorials').then((m) => m.onTutorialsDrawerOpened());
  }
}
