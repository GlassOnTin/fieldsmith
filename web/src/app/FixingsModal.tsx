import { useEffect, useMemo, useState } from 'preact/hooks';
import { store } from '../doc/store';
import type { Face, Node } from '../doc/types';
import { resolveHostSurfaceFrame } from '../doc/ops';
import { ui, useApp } from './ui';
import {
  CFRP_PEEL_COUPONS,
  FASTENER_MATERIAL_GROUPS,
  FIXINGS_CATALOGUE,
  FIXING_SIZES,
  JWST_CRYO_OPTICS_COUPONS,
  METAL_HOUSING_COUPONS,
  PLASTIC_BOSS_COUPONS,
  SUBSTRATE_MATERIAL_GROUPS,
  ensureFixingMaterialInDoc,
  evaluateFixingTorqueEnvelope,
  evaluateOpenStandardBenchmarks,
  evaluateSnapFitMechanics,
  findFixingSize,
  fitFailureDistribution,
  insertFixingIntoDocument,
  type AssociatedNutKind,
  type FailureSampleRow,
  type FixingKindId,
  type FixingPlacementMode,
} from '../doc/fixings-catalog';
import { ensureContinuumMaterialInDoc, upsertLetParam } from '../doc/continuum';

let fixingsModalOpenState = false;
let fixingsModalSeq = 0;
let initialFixingKindState: FixingKindId = 'self_tapping_screw';
let initialHostIdState = '';
let initialFaceState: Face = 'top';

export function openFixingsCatalogueModal(
  initialKind?: FixingKindId,
  targetHostId?: string,
  targetFace?: Face,
) {
  if (initialKind) {
    initialFixingKindState = initialKind;
  }
  if (targetHostId !== undefined) {
    initialHostIdState = targetHostId;
  } else if (store.selection?.kind === 'node') {
    initialHostIdState = store.selection.id;
  }
  if (targetFace) {
    initialFaceState = targetFace;
  }
  fixingsModalSeq++;
  fixingsModalOpenState = true;
  ui.emit();
}
export function closeFixingsCatalogueModal() {
  fixingsModalOpenState = false;
  ui.emit();
}

const FIXING_GROUP_PREFIXES = [
  'self_tapping_screw',
  'hex_head_screw',
  'hex_cap_bolt',
  'washer',
  'hex_nut',
  'nyloc_nut',
  'hook_clip_box',
];

function isFixingSubNode(n: Node): boolean {
  if (n.id.endsWith('_bore')) return true;
  return FIXING_GROUP_PREFIXES.some(
    (p) => n.id === p || n.id.startsWith(`${p}_`) || /^.+_\d+$/.test(n.id) && n.id.startsWith(p),
  );
}

export function FixingsCatalogueModal() {
  useApp();
  const [selectedKind, setSelectedKind] = useState<FixingKindId>(initialFixingKindState);
  const [sizeName, setSizeName] = useState<string>('M4');
  const [engagementMm, setEngagementMm] = useState<number>(8.0);
  const [fastenerMat, setFastenerMat] = useState<string>('ti64');
  const [substrateMat, setSubstrateMat] = useState<string>(store.doc.material || 'petg');
  const [vibrationG, setVibrationG] = useState<number>(12.0);
  const payloadKg = 0.15;

  // Surface Placement, Edge Dimensions & Object Tree Association state
  const [hostId, setHostId] = useState<string>(initialHostIdState);
  const [hostFace, setHostFace] = useState<Face>(initialFaceState);
  const [placementMode, setPlacementMode] = useState<FixingPlacementMode>('edge');
  const [dxEdgeInput, setDxEdgeInput] = useState<number | null>(null);
  const [dyEdgeInput, setDyEdgeInput] = useState<number | null>(null);
  const [uOffsetMm, setUOffsetMm] = useState<number>(0);
  const [vOffsetMm, setVOffsetMm] = useState<number>(0);
  const [flangeInsetXMm, setFlangeInsetXMm] = useState<number>(6.0);
  const [flangeInsetYMm, setFlangeInsetYMm] = useState<number>(6.0);
  const [boltCircleRadiusMm, setBoltCircleRadiusMm] = useState<number>(10.0);
  const [boltCircleCount, setBoltCircleCount] = useState<number>(4);
  const [associateWithHostGroup, setAssociateWithHostGroup] = useState<boolean>(true);
  const [bindSurfaceLets, setBindSurfaceLets] = useState<boolean>(true);
  const [includeWasher, setIncludeWasher] = useState<boolean>(false);
  const [associatedNut, setAssociatedNut] = useState<AssociatedNutKind>('none');

  useEffect(() => {
    if (!fixingsModalOpenState) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation();
        closeFixingsCatalogueModal();
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [fixingsModalOpenState]);

  useEffect(() => {
    if (fixingsModalOpenState) {
      setSelectedKind(initialFixingKindState);
      if (initialHostIdState) setHostId(initialHostIdState);
      setHostFace(initialFaceState);
      setDxEdgeInput(null);
      setDyEdgeInput(null);
      const getLet = (k: string): number | undefined => store.doc.lets?.find((l) => l.name === k)?.value;
      const docScrewD = getLet('screw_d') ?? getLet('bolt_d');
      if (docScrewD !== undefined) {
        const matchedSize = FIXING_SIZES.find((s) => Math.abs(s.major_d - docScrewD) < 0.25);
        if (matchedSize) setSizeName(matchedSize.name);
      }
      const docEngLen = getLet('eng_len') ?? getLet('grip_l');
      if (docEngLen !== undefined) {
        setEngagementMm(docEngLen);
      }
      if (store.doc.material) {
        setSubstrateMat(store.doc.material);
      }
    }
  }, [fixingsModalSeq]);

  if (!fixingsModalOpenState) return null;

  const doc = store.doc;
  const hostCandidates: Node[] = [];
  const collectHostCandidates = (nodes: Node[]) => {
    for (const n of nodes) {
      if (n.suppressed || n.op !== 'add' || isFixingSubNode(n)) continue;
      if (n.kind.type === 'group') {
        collectHostCandidates(n.kind.children);
      } else {
        hostCandidates.push(n);
      }
    }
  };
  collectHostCandidates(doc.root);
  const activeHostId =
    (hostId && hostCandidates.some((c) => c.id === hostId) ? hostId : undefined) ??
    (store.selection?.kind === 'node' && hostCandidates.some((c) => c.id === store.selection?.id)
      ? store.selection.id
      : undefined) ??
    hostCandidates[0]?.id ??
    '';

  const surfaceFrame = resolveHostSurfaceFrame(doc, activeHostId, hostFace);
  const dxEdgeMm = dxEdgeInput ?? Math.round(surfaceFrame.faceWidth * 0.5 * 10) / 10;
  const dyEdgeMm = dyEdgeInput ?? Math.round(surfaceFrame.faceHeight * 0.5 * 10) / 10;

  const item =
    FIXINGS_CATALOGUE.find((c) => c.id === selectedKind) ?? FIXINGS_CATALOGUE[0];
  const sizeSpec = findFixingSize(sizeName);

  const numFixings =
    placementMode === 'flange_4_corners'
      ? 4
      : placementMode === 'bolt_circle'
        ? Math.max(2, boltCircleCount)
        : 1;

  const isBoltOrScrew =
    selectedKind === 'self_tapping_screw' ||
    selectedKind === 'hex_head_screw' ||
    selectedKind === 'hex_cap_bolt';

  const envelope = evaluateFixingTorqueEnvelope({
    fixingKind: selectedKind,
    nominalDMm: sizeSpec.major_d,
    pitchMm: sizeSpec.pitch,
    engagementLengthMm: engagementMm,
    bossOdMm: sizeSpec.boss_od,
    substrateMaterial: substrateMat,
    fastenerMaterial: fastenerMat,
    vibrationG,
    shockG: vibrationG * 2.5,
    payloadMassKg: payloadKg,
    numFixings,
    deltaTempC: 0,
    serviceHours: 1000,
    targetFailureProb: 1e-3,
    muThread:
      substrateMat === 'petg' ||
      substrateMat === 'pa12' ||
      substrateMat === 'abs' ||
      substrateMat === 'pla'
        ? 0.18
        : 0.14,
    muHead:
      substrateMat === 'petg' ||
      substrateMat === 'pa12' ||
      substrateMat === 'abs' ||
      substrateMat === 'pla'
        ? 0.2
        : 0.15,
    muJoint: 0.25,
    includeWasher: isBoltOrScrew && includeWasher,
    associatedNut: isBoltOrScrew ? associatedNut : 'none',
  });

  const handleInsert = () => {
    let createdId = '';
    let collapsedIds: string[] = [];
    store.transact(`Insert ${sizeName} ${item.shortLabel} & auto-tap host`, (draft) => {
      if (substrateMat && draft.material !== substrateMat) {
        ensureFixingMaterialInDoc(draft, substrateMat);
        draft.material = substrateMat;
      }
      ensureFixingMaterialInDoc(draft, fastenerMat);
      const res = insertFixingIntoDocument(draft, {
        kind: selectedKind,
        size: sizeName,
        engagementMm,
        hostId: activeHostId || undefined,
        hostFace,
        placementMode,
        dxEdgeMm,
        dyEdgeMm,
        uOffsetMm,
        vOffsetMm,
        flangeInsetXMm,
        flangeInsetYMm,
        boltCircleRadiusMm,
        boltCircleCount,
        groupInTree: true,
        associateWithHostGroup,
        bindSurfaceLets,
        includeWasher: isBoltOrScrew && includeWasher,
        associatedNut: isBoltOrScrew ? associatedNut : 'none',
        fastenerMaterial: fastenerMat,
        substrateMaterial: substrateMat,
        vibrationG,
        payloadMassKg: payloadKg,
      });
      createdId = res.firstNewId;
      collapsedIds = res.createdGroupIds;
    });
    for (const gid of collapsedIds) {
      ui.collapsed.add(gid);
    }
    if (createdId) {
      store.select({ kind: 'node', id: createdId });
    }
    const stackDesc = [
      isBoltOrScrew && includeWasher ? '+ Washer' : '',
      isBoltOrScrew && associatedNut !== 'none'
        ? `+ ${associatedNut === 'nyloc_nut' ? 'Nyloc Nut' : 'Hex Nut'}`
        : '',
    ]
      .filter(Boolean)
      .join(' ');
    ui.notify(
      `Inserted ${numFixings > 1 ? `${numFixings}× ` : ''}${sizeName} ${item.shortLabel}${stackDesc ? ` (${stackDesc})` : ''} on ${activeHostId || 'host'}.${hostFace}: preload = ${envelope.recommendedPreloadN.toFixed(0)} N (T_rec = ${envelope.recommendedTorqueNm.toFixed(2)} N·m, F_sep = ${envelope.holdableForces.maxAxialLoadBeforeGappingN.toFixed(0)} N)`,
      'info',
    );
    closeFixingsCatalogueModal();
  };

  return (
    <div
      class="palette-scrim"
      data-testid="fixings-catalog-modal"
      onMouseDown={(e) => e.target === e.currentTarget && closeFixingsCatalogueModal()}
      style={{ zIndex: 950 }}
    >
      <div
        class="palette"
        role="dialog"
        aria-label="Engineering Fixings & Snap-Fit Geometry Catalogue"
        style={{
          width: 'min(920px, 95vw)',
          maxHeight: '88vh',
          overflowY: 'auto',
          padding: '16px 18px',
        }}
      >
        <div
          style={{
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            flexWrap: 'wrap',
            gap: '12px',
            borderBottom: '1px solid var(--line)',
            paddingBottom: '10px',
            marginBottom: '12px',
            position: 'sticky',
            top: '-16px',
            background: 'var(--panel)',
            paddingTop: '4px',
            zIndex: 6,
          }}
        >
          <div>
            <div style={{ fontSize: '15px', fontWeight: 700 }}>
              🔩 Built-In Engineering Fixings &amp; Snap-Fit Catalogue
            </div>
            <div style={{ fontSize: '12px', color: 'var(--fg-dim)', marginTop: '2px' }}>
              Choose a bolt/fixing, select the <strong>target object &amp; surface</strong> to attach it to, and position it with surface edge dimensions.
            </div>
          </div>
          <div style={{ display: 'flex', alignItems: 'center', gap: '8px' }}>
            <button
              type="button"
              class="btn primary small"
              data-testid="insert-fixing-header-btn"
              onClick={handleInsert}
              style={{ fontWeight: 700, padding: '5px 10px' }}
            >
              🔩 Attach {sizeName} {item.shortLabel} to {activeHostId || 'host'}.{hostFace}
            </button>
            <button
              class="btn ghost small"
              data-testid="close-fixings-modal"
              onClick={closeFixingsCatalogueModal}
            >
              ✕ Close
            </button>
          </div>
        </div>

        {/* 7 Fixing Type Cards (Compact) */}
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fill, minmax(195px, 1fr))',
            gap: '7px',
            marginBottom: '12px',
          }}
        >
          {FIXINGS_CATALOGUE.map((c) => {
            const active = c.id === selectedKind;
            return (
              <button
                key={c.id}
                type="button"
                data-testid={`fixing-card-${c.id}`}
                onClick={() => setSelectedKind(c.id)}
                style={{
                  textAlign: 'left',
                  padding: '8px 9px',
                  borderRadius: '8px',
                  border: active
                    ? '2px solid var(--accent, #38bdf8)'
                    : '1px solid var(--line)',
                  background: active
                    ? 'rgba(56, 189, 248, 0.12)'
                    : 'var(--bg-elev, rgba(255,255,255,0.03))',
                  color: 'inherit',
                  cursor: 'pointer',
                }}
              >
                <div
                  style={{
                    display: 'flex',
                    justifyContent: 'space-between',
                    alignItems: 'center',
                    marginBottom: '3px',
                  }}
                >
                  <span style={{ fontWeight: 700, fontSize: '12.5px' }}>
                    {c.icon} {c.shortLabel}
                  </span>
                  <span
                    style={{
                      fontSize: '9.5px',
                      padding: '1px 5px',
                      borderRadius: '4px',
                      background: 'rgba(148, 163, 184, 0.18)',
                    }}
                  >
                    {c.isoStandard}
                  </span>
                </div>
                <div style={{ fontSize: '10.5px', color: 'var(--fg-dim)', lineHeight: 1.3 }}>
                  {c.summary}
                </div>
              </button>
            );
          })}
        </div>

        {/* Target Object & Surface Attachment + Edge-Datum Dimensions & Object Tree Association */}
        <div
          data-testid="fixing-surface-placement-panel"
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))',
            gap: '12px',
            padding: '12px',
            borderRadius: '8px',
            background: 'rgba(15, 23, 42, 0.42)',
            border: '1.5px solid rgba(56, 189, 248, 0.45)',
            marginBottom: '12px',
          }}
        >
          <div style={{ display: 'flex', flexDirection: 'column', gap: '8px', fontSize: '11.5px' }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', gap: '8px', flexWrap: 'wrap' }}>
              <div style={{ fontWeight: 700, fontSize: '12.5px', color: 'var(--accent)' }}>
                🎯 Attach to Object &amp; Target Surface ({activeHostId || 'host'}.{hostFace})
              </div>
              <span style={{ fontSize: '10.5px', fontFamily: 'var(--mono)', color: 'var(--fg-dim)' }}>
                Face: {surfaceFrame.faceWidth.toFixed(1)} × {surfaceFrame.faceHeight.toFixed(1)} mm · Normal {surfaceFrame.nLabel}
              </span>
            </div>

            <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '8px' }}>
              <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
                <span style={{ fontWeight: 700 }}>1. Attach to Object (Host Part)</span>
                <select
                  data-testid="fixing-host-select"
                  value={activeHostId}
                  onChange={(e) => {
                    setHostId((e.target as HTMLSelectElement).value);
                    setDxEdgeInput(null);
                    setDyEdgeInput(null);
                  }}
                >
                  {hostCandidates.length === 0 && (
                    <option value="">(Auto-create host_housing)</option>
                  )}
                  {hostCandidates.map((n) => (
                    <option key={n.id} value={n.id}>
                      {n.id} ({n.kind.type})
                    </option>
                  ))}
                </select>
              </label>
              <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
                <span style={{ fontWeight: 700 }}>2. Attach to Surface (Face)</span>
                <select
                  data-testid="fixing-face-select"
                  value={hostFace}
                  onChange={(e) => {
                    setHostFace((e.target as HTMLSelectElement).value as Face);
                    setDxEdgeInput(null);
                    setDyEdgeInput(null);
                  }}
                >
                  <option value="top">⬆ Top (+Z)</option>
                  <option value="bottom">⬇ Bottom (−Z)</option>
                  <option value="px">➡ Right (+X)</option>
                  <option value="nx">⬅ Left (−X)</option>
                  <option value="py">↗ Back (+Y)</option>
                  <option value="ny">↙ Front (−Y)</option>
                </select>
              </label>
            </div>

            {/* Quick Object & 6-Face Pill Selector Row */}
            <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
              {hostCandidates.length > 1 && (
                <div style={{ display: 'flex', alignItems: 'center', gap: '4px', flexWrap: 'wrap' }}>
                  <span style={{ fontSize: '10px', fontWeight: 700, color: 'var(--fg-dim)', textTransform: 'uppercase' }}>
                    Object:
                  </span>
                  {hostCandidates.map((n) => (
                    <button
                      key={n.id}
                      type="button"
                      class={`btn small ${activeHostId === n.id ? 'primary' : 'ghost'}`}
                      data-testid={`fixing-host-chip-${n.id}`}
                      onClick={() => {
                        setHostId(n.id);
                        setDxEdgeInput(null);
                        setDyEdgeInput(null);
                      }}
                      style={{ fontSize: '10.5px', padding: '2px 7px' }}
                    >
                      📦 {n.id}
                    </button>
                  ))}
                </div>
              )}
              <div style={{ display: 'flex', alignItems: 'center', gap: '4px', flexWrap: 'wrap' }}>
                <span style={{ fontSize: '10px', fontWeight: 700, color: 'var(--fg-dim)', textTransform: 'uppercase' }}>
                  Surface:
                </span>
                {(
                  [
                    { f: 'top', label: '⬆ Top (+Z)' },
                    { f: 'bottom', label: '⬇ Bottom (−Z)' },
                    { f: 'px', label: '➡ Right (+X)' },
                    { f: 'nx', label: '⬅ Left (−X)' },
                    { f: 'py', label: '↗ Back (+Y)' },
                    { f: 'ny', label: '↙ Front (−Y)' },
                  ] as const
                ).map((itemFace) => (
                  <button
                    key={itemFace.f}
                    type="button"
                    class={`btn small ${hostFace === itemFace.f ? 'primary' : 'ghost'}`}
                    data-testid={`fixing-face-btn-${itemFace.f}`}
                    onClick={() => {
                      setHostFace(itemFace.f);
                      setDxEdgeInput(null);
                      setDyEdgeInput(null);
                    }}
                    style={{ fontSize: '10.5px', padding: '2px 6px' }}
                  >
                    {itemFace.label}
                  </button>
                ))}
              </div>
            </div>

            <div style={{ display: 'flex', flexDirection: 'column', gap: '4px' }}>
              <span style={{ fontWeight: 600 }}>Surface Dimension Mode</span>
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '4px' }}>
                {(
                  [
                    { id: 'edge', label: '📏 Edge Distances (dx, dy)' },
                    { id: 'center', label: '⊕ Centre Offset (u, v)' },
                    { id: 'flange_4_corners', label: '⬚ 4-Corner Flange Inset' },
                    { id: 'bolt_circle', label: '◎ Bolt Circle (PCD)' },
                  ] as const
                ).map((m) => (
                  <button
                    key={m.id}
                    type="button"
                    class={`btn small ${placementMode === m.id ? 'primary' : ''}`}
                    data-testid={`fixing-mode-${m.id}`}
                    onClick={() => setPlacementMode(m.id)}
                    style={{ fontSize: '11px', padding: '4px 6px' }}
                  >
                    {m.label}
                  </button>
                ))}
              </div>
            </div>

            {placementMode === 'edge' && (
              <>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '8px' }}>
                  <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
                    <span style={{ fontWeight: 600 }}>
                      From −{surfaceFrame.uLabel} Edge dx (mm) [0..{surfaceFrame.faceWidth.toFixed(1)}]
                    </span>
                    <input
                      type="number"
                      step="0.5"
                      data-testid="fixing-dx-edge-input"
                      value={dxEdgeMm}
                      onInput={(e) =>
                        setDxEdgeInput(Number((e.target as HTMLInputElement).value) || 0)
                      }
                    />
                  </label>
                  <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
                    <span style={{ fontWeight: 600 }}>
                      From −{surfaceFrame.vLabel} Edge dy (mm) [0..{surfaceFrame.faceHeight.toFixed(1)}]
                    </span>
                    <input
                      type="number"
                      step="0.5"
                      data-testid="fixing-dy-edge-input"
                      value={dyEdgeMm}
                      onInput={(e) =>
                        setDyEdgeInput(Number((e.target as HTMLInputElement).value) || 0)
                      }
                    />
                  </label>
                </div>
                <div style={{ display: 'flex', gap: '4px', flexWrap: 'wrap' }}>
                  <button
                    type="button"
                    class="btn ghost small"
                    data-testid="fixing-snap-center"
                    onClick={() => {
                      setDxEdgeInput(Math.round(surfaceFrame.faceWidth * 0.5 * 10) / 10);
                      setDyEdgeInput(Math.round(surfaceFrame.faceHeight * 0.5 * 10) / 10);
                    }}
                    style={{ fontSize: '10px', padding: '2px 6px' }}
                  >
                    Snap Centre
                  </button>
                  <button
                    type="button"
                    class="btn ghost small"
                    data-testid="fixing-snap-tl"
                    onClick={() => {
                      const m = Math.max(4, sizeSpec.washer_od);
                      setDxEdgeInput(m);
                      setDyEdgeInput(Math.max(m, surfaceFrame.faceHeight - m));
                    }}
                    style={{ fontSize: '10px', padding: '2px 6px' }}
                  >
                    Top-Left
                  </button>
                  <button
                    type="button"
                    class="btn ghost small"
                    data-testid="fixing-snap-tr"
                    onClick={() => {
                      const m = Math.max(4, sizeSpec.washer_od);
                      setDxEdgeInput(Math.max(m, surfaceFrame.faceWidth - m));
                      setDyEdgeInput(Math.max(m, surfaceFrame.faceHeight - m));
                    }}
                    style={{ fontSize: '10px', padding: '2px 6px' }}
                  >
                    Top-Right
                  </button>
                  <button
                    type="button"
                    class="btn ghost small"
                    data-testid="fixing-snap-bl"
                    onClick={() => {
                      const m = Math.max(4, sizeSpec.washer_od);
                      setDxEdgeInput(m);
                      setDyEdgeInput(m);
                    }}
                    style={{ fontSize: '10px', padding: '2px 6px' }}
                  >
                    Bottom-Left
                  </button>
                  <button
                    type="button"
                    class="btn ghost small"
                    data-testid="fixing-snap-br"
                    onClick={() => {
                      const m = Math.max(4, sizeSpec.washer_od);
                      setDxEdgeInput(Math.max(m, surfaceFrame.faceWidth - m));
                      setDyEdgeInput(m);
                    }}
                    style={{ fontSize: '10px', padding: '2px 6px' }}
                  >
                    Bottom-Right
                  </button>
                </div>
              </>
            )}

            {placementMode === 'center' && (
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '8px' }}>
                <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
                  <span style={{ fontWeight: 600 }}>Offset u along {surfaceFrame.uLabel} (mm)</span>
                  <input
                    type="number"
                    step="0.5"
                    data-testid="fixing-u-offset-input"
                    value={uOffsetMm}
                    onInput={(e) => setUOffsetMm(Number((e.target as HTMLInputElement).value) || 0)}
                  />
                </label>
                <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
                  <span style={{ fontWeight: 600 }}>Offset v along {surfaceFrame.vLabel} (mm)</span>
                  <input
                    type="number"
                    step="0.5"
                    data-testid="fixing-v-offset-input"
                    value={vOffsetMm}
                    onInput={(e) => setVOffsetMm(Number((e.target as HTMLInputElement).value) || 0)}
                  />
                </label>
              </div>
            )}

            {placementMode === 'flange_4_corners' && (
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '8px' }}>
                <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
                  <span style={{ fontWeight: 600 }}>Edge Inset X along {surfaceFrame.uLabel} (mm)</span>
                  <input
                    type="number"
                    step="0.5"
                    min="2"
                    data-testid="fixing-flange-inset-x"
                    value={flangeInsetXMm}
                    onInput={(e) =>
                      setFlangeInsetXMm(Math.max(2, Number((e.target as HTMLInputElement).value) || 6))
                    }
                  />
                </label>
                <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
                  <span style={{ fontWeight: 600 }}>Edge Inset Y along {surfaceFrame.vLabel} (mm)</span>
                  <input
                    type="number"
                    step="0.5"
                    min="2"
                    data-testid="fixing-flange-inset-y"
                    value={flangeInsetYMm}
                    onInput={(e) =>
                      setFlangeInsetYMm(Math.max(2, Number((e.target as HTMLInputElement).value) || 6))
                    }
                  />
                </label>
              </div>
            )}

            {placementMode === 'bolt_circle' && (
              <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '8px' }}>
                <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
                  <span style={{ fontWeight: 600 }}>Pitch Circle Radius R_pcd (mm)</span>
                  <input
                    type="number"
                    step="0.5"
                    min="3"
                    data-testid="fixing-pcd-radius"
                    value={boltCircleRadiusMm}
                    onInput={(e) =>
                      setBoltCircleRadiusMm(
                        Math.max(3, Number((e.target as HTMLInputElement).value) || 10),
                      )
                    }
                  />
                </label>
                <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
                  <span style={{ fontWeight: 600 }}>Number of Bolts N</span>
                  <input
                    type="number"
                    step="1"
                    min="2"
                    max="12"
                    data-testid="fixing-pcd-count"
                    value={boltCircleCount}
                    onInput={(e) =>
                      setBoltCircleCount(
                        Math.max(2, Math.min(12, Math.round(Number((e.target as HTMLInputElement).value) || 4))),
                      )
                    }
                  />
                </label>
              </div>
            )}

            <div
              style={{
                display: 'flex',
                flexDirection: 'column',
                gap: '4px',
                paddingTop: '4px',
                borderTop: '1px solid var(--line)',
              }}
            >
              <label style={{ display: 'flex', alignItems: 'center', gap: '6px', cursor: 'pointer' }}>
                <input
                  type="checkbox"
                  checked={associateWithHostGroup}
                  data-testid="fixing-associate-host-toggle"
                  style={{ width: '14px', height: '14px', margin: 0, flex: 'none' }}
                  onChange={(e) =>
                    setAssociateWithHostGroup((e.target as HTMLInputElement).checked)
                  }
                />
                <span>
                  Associate in Object Tree under <strong>{activeHostId || 'host'}_assy</strong> group
                </span>
              </label>
              <label style={{ display: 'flex', alignItems: 'center', gap: '6px', cursor: 'pointer' }}>
                <input
                  type="checkbox"
                  checked={bindSurfaceLets}
                  data-testid="fixing-bind-lets-toggle"
                  style={{ width: '14px', height: '14px', margin: 0, flex: 'none' }}
                  onChange={(e) => setBindSurfaceLets((e.target as HTMLInputElement).checked)}
                />
                <span>
                  Bind surface dimensions to parametric <code>let</code> variables (
                  <code>{placementMode === 'center' ? '_u, _v' : '_dx, _dy'}</code>)
                </span>
              </label>
            </div>
          </div>

          {/* Interactive 2D Surface Dimension Diagram (Click to Position Bolt on Face) */}
          {(() => {
            const fw = Math.max(10, surfaceFrame.faceWidth);
            const fh = Math.max(10, surfaceFrame.faceHeight);
            const svgW = 320;
            const svgH = 195;
            const pad = 28;
            const scale = Math.min((svgW - 2 * pad) / fw, (svgH - 2 * pad) / fh);
            const rectW = fw * scale;
            const rectH = fh * scale;
            const rx = (svgW - rectW) * 0.5;
            const ry = (svgH - rectH) * 0.5;
            const toSvgX = (dx: number) => rx + dx * scale;
            const toSvgY = (dy: number) => ry + rectH - dy * scale;

            let previewPts: { dx: number; dy: number }[] = [];
            if (placementMode === 'flange_4_corners') {
              const ix = Math.min(fw * 0.45, flangeInsetXMm);
              const iy = Math.min(fh * 0.45, flangeInsetYMm);
              previewPts = [
                { dx: ix, dy: iy },
                { dx: fw - ix, dy: iy },
                { dx: fw - ix, dy: fh - iy },
                { dx: ix, dy: fh - iy },
              ];
            } else if (placementMode === 'bolt_circle') {
              const nB = Math.max(2, boltCircleCount);
              for (let k = 0; k < nB; k++) {
                const a = (2 * Math.PI * k) / nB;
                previewPts.push({
                  dx: 0.5 * fw + boltCircleRadiusMm * Math.cos(a),
                  dy: 0.5 * fh + boltCircleRadiusMm * Math.sin(a),
                });
              }
            } else if (placementMode === 'center') {
              previewPts = [{ dx: 0.5 * fw + uOffsetMm, dy: 0.5 * fh + vOffsetMm }];
            } else {
              previewPts = [{ dx: dxEdgeMm, dy: dyEdgeMm }];
            }

            const p0 = previewPts[0] ?? { dx: 0.5 * fw, dy: 0.5 * fh };
            const px0 = toSvgX(p0.dx);
            const py0 = toSvgY(p0.dy);
            const boltR = Math.max(4, sizeSpec.cap_head_d * 0.5 * scale);
            const boreR = Math.max(2, sizeSpec.major_d * 0.5 * scale);

            return (
              <div
                style={{
                  display: 'flex',
                  flexDirection: 'column',
                  alignItems: 'center',
                  justifyContent: 'center',
                  background: 'rgba(2, 6, 23, 0.55)',
                  border: '1px solid var(--line)',
                  borderRadius: '8px',
                  padding: '8px',
                }}
              >
                <div style={{ fontSize: '11px', color: 'var(--fg-dim)', marginBottom: '4px' }}>
                  Click anywhere on <strong>{activeHostId || 'host'}.{hostFace}</strong> ({fw.toFixed(1)} × {fh.toFixed(1)} mm) to position fixing:
                </div>
                <svg
                  width={svgW}
                  height={svgH}
                  viewBox={`0 0 ${svgW} ${svgH}`}
                  data-testid="fixing-surface-diagram"
                  style={{ cursor: 'crosshair', maxWidth: '100%' }}
                  onClick={(e) => {
                    const rect = (e.currentTarget as SVGSVGElement).getBoundingClientRect();
                    const sx = ((e.clientX - rect.left) / rect.width) * svgW;
                    const sy = ((e.clientY - rect.top) / rect.height) * svgH;
                    const clickedDx = Math.max(
                      0,
                      Math.min(fw, Math.round(((sx - rx) / scale) * 2) / 2),
                    );
                    const clickedDy = Math.max(
                      0,
                      Math.min(fh, Math.round(((ry + rectH - sy) / scale) * 2) / 2),
                    );
                    if (placementMode === 'center') {
                      setUOffsetMm(Math.round((clickedDx - 0.5 * fw) * 10) / 10);
                      setVOffsetMm(Math.round((clickedDy - 0.5 * fh) * 10) / 10);
                    } else {
                      setPlacementMode('edge');
                      setDxEdgeInput(clickedDx);
                      setDyEdgeInput(clickedDy);
                    }
                  }}
                >
                  {/* Host face rectangle */}
                  <rect
                    x={rx}
                    y={ry}
                    width={rectW}
                    height={rectH}
                    fill="rgba(56, 189, 248, 0.08)"
                    stroke="#38bdf8"
                    stroke-width="1.5"
                    rx="3"
                  />
                  {/* Centre crosshairs */}
                  <line
                    x1={rx}
                    y1={ry + rectH * 0.5}
                    x2={rx + rectW}
                    y2={ry + rectH * 0.5}
                    stroke="rgba(148,163,184,0.25)"
                    stroke-dasharray="3 3"
                  />
                  <line
                    x1={rx + rectW * 0.5}
                    y1={ry}
                    x2={rx + rectW * 0.5}
                    y2={ry + rectH}
                    stroke="rgba(148,163,184,0.25)"
                    stroke-dasharray="3 3"
                  />

                  {/* Edge dimension lines for primary hole */}
                  <line
                    x1={rx}
                    y1={py0}
                    x2={px0}
                    y2={py0}
                    stroke="#fbbf24"
                    stroke-width="1.5"
                  />
                  <text
                    x={(rx + px0) * 0.5}
                    y={Math.max(14, py0 - 5)}
                    fill="#fbbf24"
                    font-size="10"
                    font-family="monospace"
                    text-anchor="middle"
                  >
                    dx={p0.dx.toFixed(1)}
                  </text>

                  <line
                    x1={px0}
                    y1={ry + rectH}
                    x2={px0}
                    y2={py0}
                    stroke="#34d399"
                    stroke-width="1.5"
                  />
                  <text
                    x={Math.min(svgW - 28, px0 + 6)}
                    y={(ry + rectH + py0) * 0.5}
                    fill="#34d399"
                    font-size="10"
                    font-family="monospace"
                  >
                    dy={p0.dy.toFixed(1)}
                  </text>

                  {/* Bolt footprint(s) */}
                  {previewPts.map((pt, idx) => {
                    const cx = toSvgX(pt.dx);
                    const cy = toSvgY(pt.dy);
                    const washerR = Math.max(boltR + 2, sizeSpec.washer_od * 0.5 * scale);
                    return (
                      <g key={idx}>
                        {isBoltOrScrew && includeWasher && (
                          <circle
                            cx={cx}
                            cy={cy}
                            r={washerR}
                            fill="rgba(56, 189, 248, 0.16)"
                            stroke="#38bdf8"
                            stroke-width="1.2"
                            stroke-dasharray="3 2"
                          />
                        )}
                        <circle
                          cx={cx}
                          cy={cy}
                          r={boltR}
                          fill="rgba(248, 250, 252, 0.22)"
                          stroke="#f8fafc"
                          stroke-width="1.4"
                        />
                        <circle
                          cx={cx}
                          cy={cy}
                          r={boreR}
                          fill="rgba(15, 23, 42, 0.9)"
                          stroke="#38bdf8"
                          stroke-width="1.2"
                        />
                      </g>
                    );
                  })}
                </svg>
                <button
                  type="button"
                  class="btn primary small"
                  data-testid="fixing-diagram-insert-btn"
                  onClick={handleInsert}
                  style={{ marginTop: '6px', width: '100%', fontWeight: 700, padding: '5px 8px' }}
                >
                  🔩 Attach {sizeName} {item.shortLabel}
                  {isBoltOrScrew && includeWasher ? ' + Washer' : ''}
                  {isBoltOrScrew && associatedNut !== 'none'
                    ? ` + ${associatedNut === 'nyloc_nut' ? 'Nyloc' : 'Hex Nut'}`
                    : ''}{' '}
                  to {activeHostId || 'host'}.{hostFace} ({placementMode === 'edge' ? `dx=${dxEdgeMm}, dy=${dyEdgeMm} mm` : placementMode === 'center' ? `u=${uOffsetMm}, v=${vOffsetMm} mm` : `${numFixings}× pattern`})
                </button>
              </div>
            );
          })()}
        </div>

        {/* Thread Size, Engagement Depth, Washer & Mated Nut Controls */}
        <div
          style={{
            display: 'flex',
            flexDirection: 'column',
            gap: '10px',
            padding: '10px 12px',
            borderRadius: '8px',
            background: 'rgba(15, 23, 42, 0.28)',
            border: '1px solid var(--line)',
            marginBottom: '12px',
          }}
        >
          <div
            style={{
              display: 'grid',
              gridTemplateColumns: 'repeat(auto-fit, minmax(155px, 1fr))',
              gap: '10px',
            }}
          >
            <label style={{ display: 'flex', flexDirection: 'column', gap: '4px', fontSize: '11.5px' }}>
              <span style={{ fontWeight: 600 }}>Nominal Thread Size</span>
              <select
                key={sizeName}
                data-testid="fixing-size-select"
                value={sizeName}
                onChange={(e) => setSizeName((e.target as HTMLSelectElement).value)}
              >
                {FIXING_SIZES.map((s) => (
                  <option key={s.name} value={s.name} selected={s.name === sizeName}>
                    {s.name} (⌀{s.major_d.toFixed(1)} × {s.pitch} mm)
                  </option>
                ))}
              </select>
            </label>

            <label style={{ display: 'flex', flexDirection: 'column', gap: '4px', fontSize: '11.5px' }}>
              <span style={{ fontWeight: 600 }}>Engaged Depth L_eng (mm)</span>
              <input
                type="number"
                step="0.5"
                min="3"
                max="25"
                data-testid="fixing-engagement-input"
                value={engagementMm}
                onInput={(e) =>
                  setEngagementMm(Math.max(3, Number((e.target as HTMLInputElement).value) || 8))
                }
              />
            </label>

            <label style={{ display: 'flex', flexDirection: 'column', gap: '4px', fontSize: '11.5px' }}>
              <span style={{ fontWeight: 600 }}>Fixed Host Material</span>
              <select
                data-testid="fixing-substrate-select"
                value={substrateMat}
                onChange={(e) => setSubstrateMat((e.target as HTMLSelectElement).value)}
              >
                {SUBSTRATE_MATERIAL_GROUPS.map((grp) => (
                  <optgroup key={grp.group} label={grp.group}>
                    {grp.options.map((opt) => (
                      <option key={opt.id} value={opt.id}>
                        {opt.label}
                      </option>
                    ))}
                  </optgroup>
                ))}
              </select>
            </label>

            <label style={{ display: 'flex', flexDirection: 'column', gap: '4px', fontSize: '11.5px' }}>
              <span style={{ fontWeight: 600 }}>Fixing Hardware Material</span>
              <select
                data-testid="fixing-hardware-select"
                value={fastenerMat}
                onChange={(e) => setFastenerMat((e.target as HTMLSelectElement).value)}
              >
                {FASTENER_MATERIAL_GROUPS.map((grp) => (
                  <optgroup key={grp.group} label={grp.group}>
                    {grp.options.map((opt) => (
                      <option key={opt.id} value={opt.id}>
                        {opt.label}
                      </option>
                    ))}
                  </optgroup>
                ))}
              </select>
            </label>

            <label style={{ display: 'flex', flexDirection: 'column', gap: '4px', fontSize: '11.5px' }}>
              <span style={{ fontWeight: 600 }}>Knock / Vibe RMS Level (g)</span>
              <input
                type="number"
                step="1"
                min="1"
                max="50"
                value={vibrationG}
                onInput={(e) =>
                  setVibrationG(Math.max(1, Number((e.target as HTMLInputElement).value) || 12))
                }
              />
            </label>
          </div>

          {/* Bolt Hardware Stack: Underhead Washer & Associated Nut */}
          {isBoltOrScrew && (
            <div
              data-testid="fixing-bolt-stack-controls"
              style={{
                display: 'grid',
                gridTemplateColumns: 'repeat(auto-fit, minmax(260px, 1fr))',
                gap: '10px',
                padding: '9px 10px',
                borderRadius: '7px',
                background: 'rgba(56, 189, 248, 0.08)',
                border: '1px solid rgba(56, 189, 248, 0.35)',
                alignItems: 'center',
              }}
            >
              <label
                style={{
                  display: 'flex',
                  alignItems: 'flex-start',
                  gap: '8px',
                  cursor: 'pointer',
                  fontSize: '11.5px',
                }}
              >
                <input
                  type="checkbox"
                  checked={includeWasher}
                  data-testid="fixing-include-washer-toggle"
                  style={{ width: '15px', height: '15px', margin: '2px 0 0 0', flex: 'none' }}
                  onChange={(e) => setIncludeWasher((e.target as HTMLInputElement).checked)}
                />
                <div style={{ display: 'flex', flexDirection: 'column', gap: '2px' }}>
                  <span style={{ fontWeight: 700 }}>
                    ⭕ Include Underhead Bearing Washer (ISO 7089 / DIN 125A)
                  </span>
                  <span style={{ fontSize: '10.5px', color: 'var(--fg-dim)', fontFamily: 'var(--mono)' }}>
                    OD ⌀{sizeSpec.washer_od.toFixed(1)} × {sizeSpec.washer_t.toFixed(1)} mm · A_brg ={' '}
                    {envelope.bearingAreaMm2.toFixed(1)} mm² · σ_brg ={' '}
                    {envelope.bearingPressureAtRecMpa.toFixed(1)} MPa · K ={' '}
                    {envelope.nutFactorK.toFixed(3)}
                  </span>
                </div>
              </label>

              <label style={{ display: 'flex', flexDirection: 'column', gap: '3px', fontSize: '11.5px' }}>
                <span style={{ fontWeight: 700 }}>⬡ Associate Mated Nut with Bolt</span>
                <select
                  data-testid="fixing-associated-nut-select"
                  value={associatedNut}
                  onChange={(e) =>
                    setAssociatedNut((e.target as HTMLSelectElement).value as AssociatedNutKind)
                  }
                >
                  <option value="none">
                    None — Tapped directly into {activeHostId || 'host'} threads
                  </option>
                  <option value="hex_nut">
                    ⬡ Hex Nut (ISO 4032 — s={sizeSpec.hex_flats_s} mm, m={sizeSpec.nut_h} mm)
                  </option>
                  <option value="nyloc_nut">
                    🔒 Nyloc Lock Nut (ISO 10511 — +Prevailing Torque T_prev, anti-vibration)
                  </option>
                </select>
              </label>
            </div>
          )}
        </div>

        {/* Live Torque Envelope & Holdable Forces Preview */}
        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(275px, 1fr))',
            gap: '10px',
            marginBottom: '12px',
          }}
        >
          {/* Card 1: Torque Requirements & Bearing Mechanics */}
          <div
            data-testid="fixing-torque-envelope-card"
            style={{
              padding: '10px 12px',
              borderRadius: '8px',
              border: '1px solid var(--line)',
              background: 'rgba(16, 185, 129, 0.08)',
              fontSize: '12px',
              display: 'flex',
              flexDirection: 'column',
              gap: '5px',
            }}
          >
            <div style={{ fontWeight: 700 }}>
              🔧 Torque Requirements [{envelope.minTorqueNm.toFixed(2)} –{' '}
              {envelope.maxTorqueNm.toFixed(2)} N·m]
            </div>
            <div
              style={{
                display: 'grid',
                gridTemplateColumns: '1fr 1fr',
                gap: '5px',
                fontFamily: 'var(--mono)',
                fontSize: '11.5px',
              }}
            >
              <div>
                T_min (Vibe/Knock): <strong>{envelope.minTorqueNm.toFixed(2)} N·m</strong>
              </div>
              <div>
                T_max (Strip/Crush): <strong>{envelope.maxTorqueNm.toFixed(2)} N·m</strong>
              </div>
              <div>
                T_rec (Target):{' '}
                <strong style={{ color: '#34d399' }}>
                  {envelope.recommendedTorqueNm.toFixed(2)} N·m
                </strong>
              </div>
              <div>
                Nut Factor K: <strong>{envelope.nutFactorK.toFixed(3)}</strong>
              </div>
              <div>
                Bearing Area A_brg: <strong>{envelope.bearingAreaMm2.toFixed(1)} mm²</strong>
              </div>
              <div>
                Bearing σ_brg: <strong>{envelope.bearingPressureAtRecMpa.toFixed(1)} MPa</strong>
              </div>
            </div>
            <div style={{ fontSize: '11px', color: 'var(--fg-dim)' }}>
              Governing Limit: <strong>{envelope.governingFailureMode}</strong> · Creep Retention:{' '}
              <strong>{(envelope.creepRetentionFactor * 100).toFixed(0)}%</strong>
            </div>
            <div
              data-testid="fixing-open-standard-provenance"
              style={{
                paddingTop: '5px',
                borderTop: '1px dashed var(--line)',
                fontSize: '10.5px',
                color: 'var(--fg-dim)',
              }}
            >
              🏛️{' '}
              <a
                href={item.openStandardUrl}
                target="_blank"
                rel="noopener noreferrer"
                style={{ color: 'var(--accent, #38bdf8)', textDecoration: 'underline' }}
              >
                {item.openStandardCode} ↗
              </a>{' '}
              ({item.isoStandard}) · Pilot ⌀
              {(envelope.substrateRegime === 'plastic_boss'
                ? sizeSpec.pilot_d_plastic
                : sizeSpec.pilot_d_metal
              ).toFixed(2)}{' '}
              mm / Clear ⌀{sizeSpec.clearance_d.toFixed(2)} mm
            </div>
          </div>

          {/* Card 2: Forces This Bolt Can Hold (Axial, Shear, Pullout, Bearing) */}
          <div
            data-testid="fixing-holdable-forces-card"
            style={{
              padding: '10px 12px',
              borderRadius: '8px',
              border: '1.5px solid rgba(56, 189, 248, 0.45)',
              background: 'rgba(56, 189, 248, 0.08)',
              fontSize: '12px',
              display: 'flex',
              flexDirection: 'column',
              gap: '5px',
            }}
          >
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <span style={{ fontWeight: 700, color: 'var(--accent)' }}>
                💪 Forces This Bolt Can Hold ({sizeName}
                {isBoltOrScrew && includeWasher ? ' + Washer' : ''}
                {isBoltOrScrew && associatedNut !== 'none'
                  ? ` + ${associatedNut === 'nyloc_nut' ? 'Nyloc' : 'Nut'}`
                  : ''}
                )
              </span>
              <span style={{ fontSize: '10px', fontFamily: 'var(--mono)', color: 'var(--fg-dim)' }}>
                1 kgf = 9.81 N
              </span>
            </div>
            <div
              style={{
                display: 'grid',
                gridTemplateColumns: '1fr 1fr',
                gap: '4px 8px',
                fontFamily: 'var(--mono)',
                fontSize: '11px',
              }}
            >
              <div title="Axial clamping preload force generated at recommended tightening torque T_rec">
                Clamp Preload F_0:{' '}
                <strong>
                  {envelope.holdableForces.axialPreloadAtRecN.toFixed(0)} N (
                  {(envelope.holdableForces.axialPreloadAtRecN / 9.80665).toFixed(1)} kgf)
                </strong>
              </div>
              <div title="Residual clamp force after viscoelastic creep and thermal ΔT">
                After Creep F_res:{' '}
                <strong>
                  {envelope.holdableForces.residualClampAfterCreepN.toFixed(0)} N (
                  {(envelope.holdableForces.residualClampAfterCreepN / 9.80665).toFixed(1)} kgf)
                </strong>
              </div>
              <div title="Max external axial tensile load before joint interface gaps (unloads clamp)">
                Axial No-Gap F_sep:{' '}
                <strong style={{ color: 'var(--accent)' }}>
                  {envelope.holdableForces.maxAxialLoadBeforeGappingN.toFixed(0)} N (
                  {(envelope.holdableForces.maxAxialLoadBeforeGappingN / 9.80665).toFixed(1)} kgf)
                </strong>
              </div>
              <div title="Max transverse shear load carried by clamp friction before joint slips">
                Friction Grip F_slip:{' '}
                <strong style={{ color: '#059669' }}>
                  {envelope.holdableForces.maxTransverseFrictionSlipN.toFixed(0)} N (
                  {(envelope.holdableForces.maxTransverseFrictionSlipN / 9.80665).toFixed(1)} kgf)
                </strong>
              </div>
              <div title="Ultimate thread stripping / pull-out force along engaged length">
                Thread Pull-Out:{' '}
                <strong>
                  {envelope.holdableForces.maxThreadPulloutN.toFixed(0)} N (
                  {(envelope.holdableForces.maxThreadPulloutN / 9.80665).toFixed(1)} kgf)
                </strong>
              </div>
              <div title="Max underhead / washer surface crush capacity before substrate yields">
                Bearing Crush:{' '}
                <strong>
                  {envelope.holdableForces.maxUnderheadBearingN.toFixed(0)} N (
                  {(envelope.holdableForces.maxUnderheadBearingN / 9.80665).toFixed(1)} kgf)
                </strong>
              </div>
              <div title="Bolt shank pin shear capacity (τ_y = 0.577·σ_y)">
                Shank Pin Shear:{' '}
                <strong>
                  {envelope.holdableForces.maxShankShearPinN.toFixed(0)} N (
                  {(envelope.holdableForces.maxShankShearPinN / 9.80665).toFixed(1)} kgf)
                </strong>
              </div>
              <div title="Safe continuous working axial / shear loads (with safety factor)">
                Safe Working ( Axial / Shear ):{' '}
                <strong style={{ color: '#d97706' }}>
                  {envelope.holdableForces.safeWorkingAxialLoadN.toFixed(0)} N /{' '}
                  {envelope.holdableForces.safeWorkingShearLoadN.toFixed(0)} N
                </strong>
              </div>
            </div>
          </div>
        </div>

        <div
          style={{
            display: 'flex',
            justifyContent: 'flex-end',
            gap: '8px',
            position: 'sticky',
            bottom: '-16px',
            background: 'var(--panel)',
            padding: '10px 0 4px',
            borderTop: '1px solid var(--line)',
            zIndex: 6,
          }}
        >
          <button class="btn ghost" onClick={closeFixingsCatalogueModal}>
            Cancel
          </button>
          <button
            class="btn primary"
            data-testid="insert-fixing-confirm"
            onClick={handleInsert}
          >
            🔩 Insert {sizeName} {item.shortLabel}
            {isBoltOrScrew && includeWasher ? ' + Washer' : ''}
            {isBoltOrScrew && associatedNut !== 'none'
              ? ` + ${associatedNut === 'nyloc_nut' ? 'Nyloc Nut' : 'Hex Nut'}`
              : ''}{' '}
            &amp; Auto-Tap Host
          </button>
        </div>
      </div>
    </div>
  );
}

// ============================================================================
// Interactive Tutorial T14 Workbench: Minimal & Maximal Torque + Spreadsheet
// Failure Distribution Fitter (Plastic vs Metal vs JWST Spaceflight Qualification)
// ============================================================================

export function FixingTorqueAndWeibullWorkbench() {
  useApp();
  const [settingPreset, setSettingPreset] = useState<'plastic' | 'metal' | 'jwst' | 'cfrp'>(
    'plastic',
  );
  const [fixingKind, setFixingKind] = useState<FixingKindId>('self_tapping_screw');
  const [sizeName, setSizeName] = useState<string>('M4');
  const [engagementMm, setEngagementMm] = useState<number>(8.0);
  const [bossOdMm, setBossOdMm] = useState<number>(10.0);
  const [substrateMat, setSubstrateMat] = useState<string>('petg');
  const [fastenerMat, setFastenerMat] = useState<string>('ti64');
  const [vibrationG, setVibrationG] = useState<number>(12.0);
  const [shockG, setShockG] = useState<number>(30.0);
  const [payloadMassKg, setPayloadMassKg] = useState<number>(0.15);
  const [deltaTempC, setDeltaTempC] = useState<number>(-20.0);
  const [serviceHours, setServiceHours] = useState<number>(1000.0);
  const [targetPf, setTargetPf] = useState<number>(1.0e-3);
  const [muThread, setMuThread] = useState<number>(0.17);
  const [muHead, setMuHead] = useState<number>(0.19);
  const [useSpreadsheetFitOverride, setUseSpreadsheetFitOverride] = useState<boolean>(true);

  const [samples, setSamples] = useState<FailureSampleRow[]>(() =>
    structuredClone(PLASTIC_BOSS_COUPONS),
  );

  const applySettingPreset = (preset: 'plastic' | 'metal' | 'jwst' | 'cfrp') => {
    setSettingPreset(preset);
    if (preset === 'plastic') {
      setFixingKind('self_tapping_screw');
      setSizeName('M4');
      setEngagementMm(8.0);
      setBossOdMm(10.0);
      setSubstrateMat('petg');
      setFastenerMat('ti64');
      setVibrationG(12.0);
      setShockG(30.0);
      setPayloadMassKg(0.15);
      setDeltaTempC(-20.0);
      setServiceHours(1000.0);
      setTargetPf(1.0e-3);
      setMuThread(0.17);
      setMuHead(0.19);
      setSamples(structuredClone(PLASTIC_BOSS_COUPONS));
    } else if (preset === 'metal') {
      setFixingKind('self_tapping_screw');
      setSizeName('M4');
      setEngagementMm(8.0);
      setBossOdMm(12.0);
      setSubstrateMat('alsi10mg');
      setFastenerMat('ti64');
      setVibrationG(18.0);
      setShockG(45.0);
      setPayloadMassKg(0.25);
      setDeltaTempC(-40.0);
      setServiceHours(5000.0);
      setTargetPf(1.0e-4);
      setMuThread(0.14);
      setMuHead(0.15);
      setSamples(structuredClone(METAL_HOUSING_COUPONS));
    } else if (preset === 'jwst') {
      setFixingKind('hex_cap_bolt');
      setSizeName('M5');
      setEngagementMm(12.0);
      setBossOdMm(15.0);
      setSubstrateMat('sic_jwst');
      setFastenerMat('ti64');
      setVibrationG(28.0);
      setShockG(65.0);
      setPayloadMassKg(0.22);
      setDeltaTempC(-255.0); // 293 K -> 38 K JWST cryo deployment
      setServiceHours(87600.0); // 10-year L2 orbit mission
      setTargetPf(1.0e-6); // 1-in-a-million spaceflight optical bench survival
      setMuThread(0.12);
      setMuHead(0.13);
      setSamples(structuredClone(JWST_CRYO_OPTICS_COUPONS));
    } else {
      setFixingKind('hex_cap_bolt');
      setSizeName('M4');
      setEngagementMm(8.0);
      setBossOdMm(12.0);
      setSubstrateMat('cf_woven_3k');
      setFastenerMat('ti64');
      setVibrationG(20.0);
      setShockG(45.0);
      setPayloadMassKg(0.2);
      setDeltaTempC(-60.0);
      setServiceHours(10000.0);
      setTargetPf(1.0e-4);
      setMuThread(0.14);
      setMuHead(0.15);
      setSamples(structuredClone(CFRP_PEEL_COUPONS));
    }
  };

  const sizeSpec = findFixingSize(sizeName);
  const distFit = useMemo(
    () => fitFailureDistribution(samples, targetPf),
    [samples, targetPf],
  );

  const envelope = useMemo(
    () =>
      evaluateFixingTorqueEnvelope({
        fixingKind,
        nominalDMm: sizeSpec.major_d,
        pitchMm: sizeSpec.pitch,
        engagementLengthMm: engagementMm,
        bossOdMm,
        substrateMaterial: substrateMat,
        fastenerMaterial: fastenerMat,
        vibrationG,
        shockG,
        payloadMassKg,
        numFixings: 1,
        deltaTempC,
        serviceHours,
        targetFailureProb: targetPf,
        muThread,
        muHead,
        muJoint: 0.25,
        weibullMOverride: useSpreadsheetFitOverride ? distFit.weibullModulusM : undefined,
        weibullSigma0OverrideMpa: useSpreadsheetFitOverride
          ? distFit.weibullScaleSigma0Mpa
          : undefined,
      }),
    [
      fixingKind,
      sizeSpec,
      engagementMm,
      bossOdMm,
      substrateMat,
      fastenerMat,
      vibrationG,
      shockG,
      payloadMassKg,
      deltaTempC,
      serviceHours,
      targetPf,
      muThread,
      muHead,
      useSpreadsheetFitOverride,
      distFit,
    ],
  );

  const snapFitReport = useMemo(
    () =>
      evaluateSnapFitMechanics(
        sizeSpec.major_d,
        engagementMm,
        substrateMat,
        muThread,
      ),
    [sizeSpec, engagementMm, substrateMat, muThread],
  );

  const openBenchmarks = useMemo(
    () =>
      evaluateOpenStandardBenchmarks(
        {
          fixingKind,
          nominalDMm: sizeSpec.major_d,
          pitchMm: sizeSpec.pitch,
          engagementLengthMm: engagementMm,
          bossOdMm,
          substrateMaterial: substrateMat,
          fastenerMaterial: fastenerMat,
          vibrationG,
          shockG,
          payloadMassKg,
          numFixings: 1,
          deltaTempC,
          serviceHours,
          targetFailureProb: targetPf,
          muThread,
          muHead,
          muJoint: 0.25,
          weibullMOverride: useSpreadsheetFitOverride ? distFit.weibullModulusM : undefined,
          weibullSigma0OverrideMpa: useSpreadsheetFitOverride
            ? distFit.weibullScaleSigma0Mpa
            : undefined,
        },
        samples,
        targetPf,
      ),
    [
      fixingKind,
      sizeSpec,
      engagementMm,
      bossOdMm,
      substrateMat,
      fastenerMat,
      vibrationG,
      shockG,
      payloadMassKg,
      deltaTempC,
      serviceHours,
      targetPf,
      muThread,
      muHead,
      useSpreadsheetFitOverride,
      distFit,
      samples,
    ],
  );

  const syncTo3dCad = () => {
    const pilotD =
      envelope.substrateRegime === 'plastic_boss'
        ? sizeSpec.pilot_d_plastic
        : sizeSpec.pilot_d_metal;
    const pilotR = Number((pilotD * 0.5).toFixed(2));
    const bossR = Number((bossOdMm * 0.5).toFixed(2));
    const preloadN = Number(envelope.recommendedPreloadN.toFixed(1));
    const torqueNm = Number(envelope.recommendedTorqueNm.toFixed(3));

    store.transact(
      `Sync ${sizeName} ${substrateMat.toUpperCase()} fixing envelope (T_rec = ${torqueNm} N·m, F = ${preloadN} N)`,
      (draft) => {
        ensureContinuumMaterialInDoc(draft, substrateMat);
        ensureContinuumMaterialInDoc(draft, fastenerMat);
        draft.material = substrateMat;
        upsertLetParam(draft, 'screw_d', sizeSpec.major_d);
        upsertLetParam(draft, 'screw_pitch', sizeSpec.pitch);
        upsertLetParam(draft, 'pilot_r', pilotR);
        upsertLetParam(draft, 'boss_r', bossR);
        upsertLetParam(draft, 'eng_len', engagementMm);
        upsertLetParam(draft, 'torque_nm', torqueNm);
        upsertLetParam(draft, 'preload_n', preloadN);

        // Update existing load preload if present
        const preloadLoad = draft.loads.find((l) => l.kind.type === 'preload');
        if (preloadLoad && preloadLoad.kind.type === 'preload') {
          preloadLoad.kind.newtons = preloadN;
        }
      },
    );
    ui.notify(
      `Synced to 3D CAD: ${substrateMat.toUpperCase()} · T_min = ${envelope.minTorqueNm.toFixed(2)} N·m, T_max = ${envelope.maxTorqueNm.toFixed(2)} N·m, Preload = ${preloadN} N`,
      'info',
    );
  };

  const updateSampleCell = (
    idx: number,
    field: 'id' | 'torqueNm' | 'principalStressMpa' | 'failureProb' | 'modeNote',
    raw: string,
  ) => {
    setSamples((prev) => {
      const next = structuredClone(prev);
      const row = next[idx];
      if (!row) return prev;
      if (field === 'id') row.id = raw;
      else if (field === 'modeNote') row.modeNote = raw;
      else if (field === 'torqueNm') {
        const v = parseFloat(raw);
        if (Number.isFinite(v) && v > 0) row.torqueNm = v;
      } else if (field === 'principalStressMpa') {
        const v = parseFloat(raw);
        if (Number.isFinite(v) && v > 0) row.principalStressMpa = v;
      } else if (field === 'failureProb') {
        if (raw.trim() === '') row.failureProb = null;
        else {
          const v = parseFloat(raw);
          if (Number.isFinite(v)) row.failureProb = Math.min(0.9999, Math.max(0.0001, v / 100));
        }
      }
      return next;
    });
  };

  const addSampleRow = () => {
    setSamples((prev) => {
      const last = prev[prev.length - 1];
      const nextIdx = prev.length + 1;
      return [
        ...prev,
        {
          id: `S-${nextIdx}`,
          torqueNm: Number(((last?.torqueNm ?? 1.8) * 1.06).toFixed(2)),
          principalStressMpa: Number(((last?.principalStressMpa ?? 58) * 1.06).toFixed(1)),
          failureProb: null,
          modeNote: 'Empirical coupon test',
        },
      ];
    });
  };

  const removeSampleRow = (idx: number) => {
    if (samples.length <= 2) return;
    setSamples((prev) => prev.filter((_, i) => i !== idx));
  };

  const windowFeasible = envelope.maxTorqueNm > envelope.minTorqueNm;

  return (
    <div
      data-testid="t14-fixing-workbench"
      style={{
        marginTop: '12px',
        padding: '12px',
        borderRadius: '10px',
        border: '1px solid var(--line)',
        background: 'rgba(15, 23, 42, 0.35)',
        display: 'flex',
        flexDirection: 'column',
        gap: '12px',
      }}
    >
      {/* Top Preset Regime Selector */}
      <div
        style={{
          display: 'flex',
          flexWrap: 'wrap',
          justifyContent: 'space-between',
          alignItems: 'center',
          gap: '8px',
        }}
      >
        <div style={{ fontWeight: 700, fontSize: '13px' }}>
          🔩 Interactive Fixing Torque Envelope & Weibull/Creep Failure Lab
        </div>
        <div style={{ display: 'flex', flexWrap: 'wrap', gap: '6px' }}>
          <button
            type="button"
            class={`btn small ${settingPreset === 'plastic' ? 'primary' : 'ghost'}`}
            data-testid="t14-preset-plastic"
            onClick={() => applySettingPreset('plastic')}
          >
            1. Plastic Boss (PETG/PA12)
          </button>
          <button
            type="button"
            class={`btn small ${settingPreset === 'metal' ? 'primary' : 'ghost'}`}
            data-testid="t14-preset-metal"
            onClick={() => applySettingPreset('metal')}
          >
            2. Metal Housing (AlSi10Mg)
          </button>
          <button
            type="button"
            class={`btn small ${settingPreset === 'jwst' ? 'primary' : 'ghost'}`}
            data-testid="t14-preset-jwst"
            onClick={() => applySettingPreset('jwst')}
          >
            3. JWST Cryo-Optics (38 K, P_f ≤ 10⁻⁶)
          </button>
          <button
            type="button"
            class={`btn small ${settingPreset === 'cfrp' ? 'primary' : 'ghost'}`}
            data-testid="t14-preset-cfrp"
            onClick={() => applySettingPreset('cfrp')}
          >
            4. 3K Woven CFRP (Tsai–Wu)
          </button>
          <button
            type="button"
            class="btn small ghost"
            data-testid="t14-open-catalog-btn"
            onClick={() => openFixingsCatalogueModal(fixingKind)}
          >
            📦 Open Fixings Catalogue…
          </button>
        </div>
      </div>

      {/* Sliding-Surface & Environment Sliders */}
      <div
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(150px, 1fr))',
          gap: '8px',
          fontSize: '11.5px',
        }}
      >
        <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
          <span>Fixing Type</span>
          <select
            data-testid="t14-fixing-kind"
            value={fixingKind}
            onChange={(e) => setFixingKind((e.target as HTMLSelectElement).value as FixingKindId)}
          >
            {FIXINGS_CATALOGUE.map((f) => (
              <option key={f.id} value={f.id}>
                {f.shortLabel}
              </option>
            ))}
          </select>
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
          <span>Thread Size</span>
          <select
            data-testid="t14-size-select"
            value={sizeName}
            onChange={(e) => setSizeName((e.target as HTMLSelectElement).value)}
          >
            {FIXING_SIZES.map((s) => (
              <option key={s.name} value={s.name}>
                {s.name} (d={s.major_d} P={s.pitch})
              </option>
            ))}
          </select>
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
          <span>Substrate Material</span>
          <select
            data-testid="t14-substrate-select"
            value={substrateMat}
            onChange={(e) => setSubstrateMat((e.target as HTMLSelectElement).value)}
          >
            {SUBSTRATE_MATERIAL_GROUPS.map((grp) => (
              <optgroup key={grp.group} label={grp.group}>
                {grp.options.map((opt) => (
                  <option key={opt.id} value={opt.id}>
                    {opt.label}
                  </option>
                ))}
              </optgroup>
            ))}
          </select>
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
          <span>Fastener Material</span>
          <select
            data-testid="t14-fastener-select"
            value={fastenerMat}
            onChange={(e) => setFastenerMat((e.target as HTMLSelectElement).value)}
          >
            {FASTENER_MATERIAL_GROUPS.map((grp) => (
              <optgroup key={grp.group} label={grp.group}>
                {grp.options.map((opt) => (
                  <option key={opt.id} value={opt.id}>
                    {opt.label}
                  </option>
                ))}
              </optgroup>
            ))}
          </select>
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
          <span>Engaged Depth L_eng: {engagementMm.toFixed(1)} mm</span>
          <input
            type="range"
            min="2"
            max="20"
            step="0.5"
            data-testid="t14-engagement-slider"
            value={engagementMm}
            onInput={(e) => setEngagementMm(Number((e.target as HTMLInputElement).value))}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
          <span>Vibration / Knock: {vibrationG} g</span>
          <input
            type="range"
            min="1"
            max="45"
            step="1"
            data-testid="t14-vibration-slider"
            value={vibrationG}
            onInput={(e) => {
              const g = Number((e.target as HTMLInputElement).value);
              setVibrationG(g);
              setShockG(Math.round(g * 2.5));
            }}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
          <span>Thread Friction μ_t: {muThread.toFixed(2)}</span>
          <input
            type="range"
            min="0.08"
            max="0.32"
            step="0.01"
            data-testid="t14-friction-slider"
            value={muThread}
            onInput={(e) => setMuThread(Number((e.target as HTMLInputElement).value))}
          />
        </label>

        <label style={{ display: 'flex', flexDirection: 'column', gap: '3px' }}>
          <span>Target Reliability P_f</span>
          <select
            data-testid="t14-target-pf"
            value={String(targetPf)}
            onChange={(e) => setTargetPf(Number((e.target as HTMLSelectElement).value))}
          >
            <option value="0.01">10⁻² (1% Consumer Grade)</option>
            <option value="0.001">10⁻³ (0.1% Industrial)</option>
            <option value="0.0001">10⁻⁴ (0.01% Automotive/Aero)</option>
            <option value="0.000001">10⁻⁶ (1 ppm JWST Spaceflight)</option>
          </select>
        </label>
      </div>

      {/* Infeasible Torque Window Warning (T_min > T_max) */}
      {!windowFeasible && (
        <div
          data-testid="t14-infeasible-window-warning"
          style={{
            padding: '8px 12px',
            borderRadius: '8px',
            background: 'rgba(239, 68, 68, 0.18)',
            border: '1px solid rgba(239, 68, 68, 0.65)',
            color: '#fecaca',
            fontSize: '11.5px',
            lineHeight: 1.4,
          }}
        >
          <strong>
            ⚠️ Infeasible Torque Window (T_min = {envelope.minTorqueNm.toFixed(3)} N·m &gt; T_max ={' '}
            {envelope.maxTorqueNm.toFixed(3)} N·m):
          </strong>{' '}
          The torque required to prevent Junker vibration slip and thread-forming exceeds the{' '}
          <strong>{envelope.governingFailureMode}</strong> limit. Increase engaged depth{' '}
          <code>L_eng</code>, step up nominal thread size, add a helical wire thread insert, or
          share the payload across multiple fixings.
        </div>
      )}

      {/* VDI 2251 Cantilever Hook-Clip Snap-Fit Mechanics Banner when hook_clip_box is selected */}
      {fixingKind === 'hook_clip_box' && (
        <div
          data-testid="t14-snapfit-mechanics"
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(175px, 1fr))',
            gap: '8px',
            padding: '9px 12px',
            borderRadius: '8px',
            background: 'rgba(56, 189, 248, 0.12)',
            border: '1px solid rgba(56, 189, 248, 0.45)',
            fontSize: '11.5px',
          }}
        >
          <div>
            <div style={{ fontSize: '10px', color: 'var(--fg-dim)' }}>
              VDI 2251 CANTILEVER SNAP-FIT BEAM
            </div>
            <div style={{ fontWeight: 700, fontFamily: 'var(--mono)' }}>
              L={snapFitReport.beamLengthMm.toFixed(1)} × b={snapFitReport.beamWidthMm.toFixed(1)} × h=
              {snapFitReport.beamThicknessMm.toFixed(2)} mm
            </div>
            <div style={{ fontSize: '10px', color: 'var(--fg-dim)' }}>
              Undercut y = {snapFitReport.undercutDepthMm.toFixed(2)} mm (α=30°, β=65°)
            </div>
          </div>
          <div>
            <div style={{ fontSize: '10px', color: 'var(--fg-dim)' }}>
              PUSH-ON INSERTION / PULL-OFF RETENTION
            </div>
            <div style={{ fontWeight: 700, fontFamily: 'var(--mono)' }}>
              F_push = {snapFitReport.insertionForcePushN.toFixed(1)} N · F_pull ={' '}
              {snapFitReport.retentionForcePullN.toFixed(1)} N
            </div>
            <div style={{ fontSize: '10px', color: 'var(--fg-dim)' }}>
              Asymmetric wedge ratio F_pull/F_push ={' '}
              {(snapFitReport.retentionForcePullN / Math.max(0.1, snapFitReport.insertionForcePushN)).toFixed(2)}×
            </div>
          </div>
          <div>
            <div style={{ fontSize: '10px', color: 'var(--fg-dim)' }}>
              EULER–BERNOULLI ROOT FLEXURAL STRAIN
            </div>
            <div
              style={{
                fontWeight: 700,
                fontFamily: 'var(--mono)',
                color:
                  snapFitReport.rootFlexuralStrainPct <= snapFitReport.allowableStrainPct
                    ? '#34d399'
                    : '#f87171',
              }}
            >
              ε_root = {snapFitReport.rootFlexuralStrainPct.toFixed(2)}% (Allow ≤{' '}
              {snapFitReport.allowableStrainPct.toFixed(2)}%)
            </div>
            <div style={{ fontSize: '10px', color: 'var(--fg-dim)' }}>
              Strain Safety Factor = {snapFitReport.strainSafetyFactor.toFixed(2)}×
            </div>
          </div>
        </div>
      )}

      {/* Live Minimal & Maximal Torque Readout Banner */}
      <div
        data-testid="t14-torque-envelope"
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(185px, 1fr))',
          gap: '8px',
          padding: '10px 12px',
          borderRadius: '8px',
          background: windowFeasible
            ? 'rgba(16, 185, 129, 0.10)'
            : 'rgba(239, 68, 68, 0.14)',
          border: windowFeasible
            ? '1px solid rgba(16, 185, 129, 0.4)'
            : '1px solid rgba(239, 68, 68, 0.5)',
        }}
      >
        <div>
          <div style={{ fontSize: '10.5px', color: 'var(--fg-dim)' }}>
            MINIMAL TORQUE T_min (Knock/Vibe Slip)
          </div>
          <div
            data-testid="t14-min-torque-val"
            style={{ fontSize: '16px', fontWeight: 700, fontFamily: 'var(--mono)' }}
          >
            {envelope.minTorqueNm.toFixed(3)} N·m
          </div>
          <div style={{ fontSize: '10.5px', color: 'var(--fg-dim)' }}>
            F_clamp,min = {envelope.minResidualClampN.toFixed(0)} N · T_form ={' '}
            {envelope.threadFormingTorqueNm.toFixed(2)} N·m · Creep η ={' '}
            {(envelope.creepRetentionFactor * 100).toFixed(0)}%
          </div>
        </div>

        <div>
          <div style={{ fontSize: '10.5px', color: 'var(--fg-dim)' }}>
            MAXIMAL TORQUE T_max (Material Limit)
          </div>
          <div
            data-testid="t14-max-torque-val"
            style={{ fontSize: '16px', fontWeight: 700, fontFamily: 'var(--mono)' }}
          >
            {envelope.maxTorqueNm.toFixed(3)} N·m
          </div>
          <div
            data-testid="t14-governing-mode"
            style={{ fontSize: '10.5px', color: 'var(--fg-dim)' }}
          >
            Mode: <strong>{envelope.governingFailureMode}</strong>
          </div>
        </div>

        <div>
          <div style={{ fontSize: '10.5px', color: 'var(--fg-dim)' }}>
            {windowFeasible ? 'RECOMMENDED ASSEMBLY SETPOINT' : 'INFEASIBLE WINDOW (T_min > T_max)'}
          </div>
          <div
            data-testid="t14-rec-torque-val"
            style={{ fontSize: '16px', fontWeight: 700, fontFamily: 'var(--mono)' }}
          >
            {envelope.recommendedTorqueNm.toFixed(3)} N·m ({envelope.recommendedPreloadN.toFixed(0)} N)
          </div>
          <div style={{ fontSize: '10.5px', color: 'var(--fg-dim)' }}>
            Window Ratio T_max/T_min = {envelope.torqueWindowRatio.toFixed(2)}× · σ₁ ={' '}
            {envelope.peakPrincipalStressMpa.toFixed(1)} MPa
          </div>
        </div>

        <div style={{ display: 'flex', flexDirection: 'column', justifyContent: 'center', gap: '4px' }}>
          <button
            type="button"
            class="btn primary small"
            data-testid="t14-sync-cad"
            onClick={syncTo3dCad}
          >
            🔄 Sync Torque & Preload to 3D .fsm
          </button>
          <span style={{ fontSize: '10px', color: 'var(--fg-dim)', textAlign: 'center' }}>
            Motosh Nut Factor K = {envelope.nutFactorK.toFixed(3)}
          </span>
        </div>
      </div>

      {/* Spreadsheet-Like Failure Sample Probability Fitter + Live Weibull Plot */}
      <div
        data-testid="t14-failure-spreadsheet"
        style={{
          display: 'grid',
          gridTemplateColumns: 'repeat(auto-fit, minmax(280px, 1fr))',
          gap: '10px',
          alignItems: 'start',
        }}
      >
        <div
          style={{
            border: '1px solid var(--line)',
            borderRadius: '8px',
            padding: '10px',
            background: 'rgba(15, 23, 42, 0.25)',
          }}
        >
          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
              marginBottom: '6px',
              flexWrap: 'wrap',
              gap: '6px',
            }}
          >
            <div>
              <div style={{ fontWeight: 700, fontSize: '12px', display: 'flex', alignItems: 'center', gap: '6px', flexWrap: 'wrap' }}>
                <span>📊 Empirical Failure Sample Probability Spreadsheet (Weibull / Creep Fitter)</span>
                <span
                  style={{
                    fontSize: '9.5px',
                    padding: '1px 6px',
                    borderRadius: '4px',
                    background: 'rgba(56, 189, 248, 0.15)',
                    color: '#38bdf8',
                    fontWeight: 600,
                  }}
                >
                  Synthetic Reference Coupons (Editable)
                </span>
              </div>
              <div style={{ fontSize: '10.5px', color: 'var(--fg-dim)' }}>
                Edit coupon torque, peak principal stress σ₁, or failure probability P_f (%) to fit Weibull modulus m and scale σ₀ in real time.
              </div>
            </div>
            <div style={{ display: 'flex', gap: '6px', alignItems: 'center', flexWrap: 'wrap' }}>
              <label style={{ fontSize: '11px', display: 'flex', alignItems: 'center', gap: '4px' }}>
                <input
                  type="checkbox"
                  checked={useSpreadsheetFitOverride}
                  onChange={(e) =>
                    setUseSpreadsheetFitOverride((e.target as HTMLInputElement).checked)
                  }
                />
                Drive T_max from Fit
              </label>
              <button
                type="button"
                class="btn small ghost"
                data-testid="t14-add-sample-row"
                onClick={addSampleRow}
              >
                + Add Sample Row
              </button>
            </div>
          </div>

          {useSpreadsheetFitOverride &&
            !envelope.governingFailureMode.startsWith('Weibull') && (
              <div
                style={{
                  fontSize: '10px',
                  color: '#fbbf24',
                  background: 'rgba(245, 158, 11, 0.10)',
                  border: '1px solid rgba(245, 158, 11, 0.35)',
                  borderRadius: '4px',
                  padding: '3px 7px',
                  marginBottom: '6px',
                }}
              >
                ℹ️ Fitted Weibull limit is{' '}
                <strong>{envelope.modeLimitsNm.weibullPrincipalFractureNm.toFixed(2)} N·m</strong>,
                while <strong>{envelope.governingFailureMode}</strong> occurs earlier at{' '}
                <strong>{envelope.maxTorqueNm.toFixed(2)} N·m</strong> and therefore governs T_max.
              </div>
            )}

          <div style={{ overflowX: 'auto' }}>
            <table
              style={{
                width: '100%',
                borderCollapse: 'collapse',
                fontSize: '11px',
                fontFamily: 'var(--mono)',
              }}
            >
              <thead>
                <tr style={{ borderBottom: '1px solid var(--line)', textAlign: 'left' }}>
                  <th style={{ padding: '4px' }}>Coupon</th>
                  <th style={{ padding: '4px' }}>Torque T (N·m)</th>
                  <th style={{ padding: '4px' }}>Peak σ₁ (MPa)</th>
                  <th style={{ padding: '4px' }}>Failure Prob P_f (%)</th>
                  <th style={{ padding: '4px' }}>ln(ln(1/(1−P_f)))</th>
                  <th style={{ padding: '4px' }}>Failure Mode</th>
                  <th style={{ padding: '4px' }}></th>
                </tr>
              </thead>
              <tbody>
                {samples.map((row, idx) => {
                  const medianRankPct = ((idx + 1 - 0.3) / (samples.length + 0.4)) * 100;
                  const pfFrac =
                    row.failureProb != null ? row.failureProb : medianRankPct / 100;
                  const weibullY = Math.log(
                    -Math.log(1 - Math.min(0.9999, Math.max(0.0001, pfFrac))),
                  );
                  return (
                    <tr
                      key={idx}
                      data-testid={`t14-sample-row-${idx}`}
                      style={{ borderBottom: '1px solid rgba(148,163,184,0.12)' }}
                    >
                      <td style={{ padding: '3px 4px' }}>
                        <input
                          type="text"
                          value={row.id}
                          style={{ width: '58px', fontSize: '11px' }}
                          onInput={(e) =>
                            updateSampleCell(idx, 'id', (e.target as HTMLInputElement).value)
                          }
                        />
                      </td>
                      <td style={{ padding: '3px 4px' }}>
                        <input
                          type="number"
                          step="0.05"
                          data-testid={`t14-row-torque-${idx}`}
                          value={row.torqueNm}
                          style={{ width: '64px', fontSize: '11px' }}
                          onInput={(e) =>
                            updateSampleCell(idx, 'torqueNm', (e.target as HTMLInputElement).value)
                          }
                        />
                      </td>
                      <td style={{ padding: '3px 4px' }}>
                        <input
                          type="number"
                          step="0.5"
                          data-testid={`t14-row-stress-${idx}`}
                          value={row.principalStressMpa}
                          style={{ width: '68px', fontSize: '11px' }}
                          onInput={(e) =>
                            updateSampleCell(
                              idx,
                              'principalStressMpa',
                              (e.target as HTMLInputElement).value,
                            )
                          }
                        />
                      </td>
                      <td style={{ padding: '3px 4px' }}>
                        <input
                          type="number"
                          step="1"
                          min="0.01"
                          max="99.9"
                          data-testid={`t14-row-pf-${idx}`}
                          placeholder={medianRankPct.toFixed(1)}
                          value={
                            row.failureProb != null
                              ? Number((row.failureProb * 100).toFixed(2))
                              : ''
                          }
                          style={{ width: '64px', fontSize: '11px' }}
                          onInput={(e) =>
                            updateSampleCell(
                              idx,
                              'failureProb',
                              (e.target as HTMLInputElement).value,
                            )
                          }
                        />
                      </td>
                      <td style={{ padding: '3px 4px', color: 'var(--fg-dim)' }}>
                        {weibullY.toFixed(2)}
                      </td>
                      <td style={{ padding: '3px 4px' }}>
                        <input
                          type="text"
                          value={row.modeNote}
                          style={{ width: '125px', fontSize: '10.5px' }}
                          onInput={(e) =>
                            updateSampleCell(idx, 'modeNote', (e.target as HTMLInputElement).value)
                          }
                        />
                      </td>
                      <td style={{ padding: '3px 4px' }}>
                        <button
                          type="button"
                          class="btn ghost mini"
                          title="Remove sample row"
                          onClick={() => removeSampleRow(idx)}
                        >
                          ✕
                        </button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        </div>

        {/* Fitted Weibull Probability Plot & Mode Comparison */}
        <div
          style={{
            border: '1px solid var(--line)',
            borderRadius: '8px',
            padding: '10px',
            background: 'rgba(15, 23, 42, 0.25)',
            display: 'flex',
            flexDirection: 'column',
            gap: '8px',
          }}
        >
          <div style={{ fontWeight: 700, fontSize: '12px' }}>
            📈 Fitted 2-Parameter Weibull Linear Regression (R² ={' '}
            {(distFit.weibullR2 * 100).toFixed(2)}%)
          </div>
          <div
            data-testid="t14-weibull-fit-summary"
            style={{
              display: 'grid',
              gridTemplateColumns: '1fr 1fr',
              gap: '4px',
              fontFamily: 'var(--mono)',
              fontSize: '11px',
            }}
          >
            <div>
              Weibull Modulus m: <strong>{distFit.weibullModulusM.toFixed(2)}</strong>
            </div>
            <div>
              Scale σ₀ (63.2%): <strong>{distFit.weibullScaleSigma0Mpa.toFixed(1)} MPa</strong>
            </div>
            <div>
              Torque Scale T₀: <strong>{distFit.weibullScaleT0Nm.toFixed(2)} N·m</strong>
            </div>
            <div>
              Allowable σ₁(P_f):{' '}
              <strong>{distFit.allowableStressAtTargetPfMpa.toFixed(1)} MPa</strong>
            </div>
          </div>

          <WeibullProbabilityPlot fit={distFit} targetPf={targetPf} />

          <div style={{ fontSize: '10.5px', color: 'var(--fg-dim)', lineHeight: 1.35 }}>
            <strong>Candidate Upper Torque Limits by Failure Mode:</strong>
            <div
              style={{
                display: 'grid',
                gridTemplateColumns: '1fr auto',
                gap: '2px 8px',
                fontFamily: 'var(--mono)',
                marginTop: '3px',
              }}
            >
              <span>• Weibull Principal Fracture (P_f ≤ {targetPf.toExponential(0)}):</span>
              <span>{envelope.modeLimitsNm.weibullPrincipalFractureNm.toFixed(2)} N·m</span>
              <span>• Boss Viscoplastic Creep / Lamé Hoop Split:</span>
              <span>{envelope.modeLimitsNm.bossCreepOrHoopSplitNm.toFixed(2)} N·m</span>
              <span>• Internal Substrate Thread Shear Strip:</span>
              <span>{envelope.modeLimitsNm.substrateThreadStripNm.toFixed(2)} N·m</span>
              <span>• Fastener Core Tension–Torsion Yield:</span>
              <span>{envelope.modeLimitsNm.fastenerCoreYieldNm.toFixed(2)} N·m</span>
              <span>• Woven CFRP Tsai–Wu Interlaminar Peel:</span>
              <span>{envelope.modeLimitsNm.compositeInterlaminarPeelNm.toFixed(2)} N·m</span>
            </div>
          </div>
        </div>
      </div>

      {/* Dual-Mode Open Standards Comparator: (a) Quick Standard Lookup vs (b) First-Principles A-Priori Rederivation */}
      <div
        data-testid="t14-open-standards-comparator"
        style={{
          border: '1px solid var(--line)',
          borderRadius: '8px',
          padding: '10px',
          background: 'rgba(15, 23, 42, 0.28)',
          display: 'flex',
          flexDirection: 'column',
          gap: '8px',
        }}
      >
        <div
          style={{
            display: 'flex',
            justifyContent: 'space-between',
            alignItems: 'center',
            flexWrap: 'wrap',
            gap: '6px',
          }}
        >
          <div>
            <div style={{ fontWeight: 700, fontSize: '12px' }}>
              🏛️ Open Public Standards Comparator: (a) Quick Standard Rule vs. (b) First-Principles A-Priori Rederivation
            </div>
            <div style={{ fontSize: '10.5px', color: 'var(--fg-dim)' }}>
              Unencumbered US Federal / NASA / NIST standards reproduced alongside their continuum &amp; statistical derivations (replaces paywalled ISO/VDI tables).
            </div>
          </div>
        </div>

        <div
          style={{
            display: 'grid',
            gridTemplateColumns: 'repeat(auto-fit, minmax(260px, 1fr))',
            gap: '8px',
          }}
        >
          {openBenchmarks.map((b) => {
            const deltaColor =
              b.discrepancyPct <= 2.5
                ? '#34d399'
                : b.discrepancyPct <= 12.0
                  ? '#fbbf24'
                  : '#f87171';
            return (
              <div
                key={b.id}
                data-testid={`t14-std-card-${b.id}`}
                style={{
                  border: '1px solid rgba(148, 163, 184, 0.22)',
                  borderRadius: '6px',
                  padding: '8px 10px',
                  background: 'rgba(15, 23, 42, 0.35)',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: '4px',
                  fontSize: '11px',
                }}
              >
                <div
                  style={{
                    display: 'flex',
                    justifyContent: 'space-between',
                    alignItems: 'center',
                    gap: '6px',
                  }}
                >
                  <a
                    href={b.openUrl}
                    target="_blank"
                    rel="noopener noreferrer"
                    style={{
                      fontWeight: 700,
                      color: 'var(--accent, #38bdf8)',
                      textDecoration: 'underline',
                      fontSize: '11px',
                    }}
                  >
                    {b.standardCode} ↗
                  </a>
                  <span
                    style={{
                      fontSize: '10px',
                      padding: '1px 5px',
                      borderRadius: '4px',
                      background: 'rgba(148, 163, 184, 0.16)',
                      color: 'var(--fg-dim)',
                    }}
                  >
                    {b.isoCrossRef}
                  </span>
                </div>
                <div style={{ fontWeight: 600, fontSize: '11.5px' }}>{b.title}</div>
                <div
                  style={{
                    display: 'grid',
                    gridTemplateColumns: '1fr 1fr auto',
                    gap: '6px',
                    padding: '5px 6px',
                    borderRadius: '4px',
                    background: 'rgba(16, 185, 129, 0.09)',
                    fontFamily: 'var(--mono)',
                    fontSize: '11px',
                    alignItems: 'center',
                  }}
                >
                  <div>
                    <div style={{ fontSize: '9.5px', color: 'var(--fg-dim)' }}>
                      (a) Quick Standard
                    </div>
                    <strong>{b.standardValue.toFixed(3)}</strong>
                  </div>
                  <div>
                    <div style={{ fontSize: '9.5px', color: 'var(--fg-dim)' }}>
                      (b) A-Priori Derived
                    </div>
                    <strong>{b.aprioriValue.toFixed(3)}</strong>
                  </div>
                  <div style={{ textAlign: 'right' }}>
                    <div style={{ fontSize: '9.5px', color: 'var(--fg-dim)' }}>Δ</div>
                    <span style={{ color: deltaColor, fontWeight: 700 }}>
                      {b.discrepancyPct.toFixed(2)}%
                    </span>
                  </div>
                </div>
                <div style={{ fontSize: '10px', color: 'var(--fg-dim)', lineHeight: 1.35 }}>
                  <div>
                    <strong>(a) Standard Rule:</strong> {b.quickStandardRule}
                  </div>
                  <div>
                    <strong>(b) A-Priori Foundation:</strong> {b.aprioriDerivation}
                  </div>
                  <div style={{ marginTop: '2px', color: 'var(--fg)' }}>{b.regimeInsight}</div>
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

function WeibullProbabilityPlot({
  fit,
  targetPf,
}: {
  fit: ReturnType<typeof fitFailureDistribution>;
  targetPf: number;
}) {
  const w = 310;
  const h = 145;
  const padL = 36;
  const padR = 14;
  const padT = 12;
  const padB = 24;

  const pts = fit.points;
  const m = fit.weibullModulusM;
  const lnSig0 = Math.log(Math.max(1, fit.weibullScaleSigma0Mpa));
  const rawTargetY = Math.log(-Math.log(1 - targetPf));
  const targetLnStress = lnSig0 + rawTargetY / Math.max(0.5, m);

  const sampleXMin = pts.length > 0 ? Math.min(...pts.map((p) => p.lnStress)) : 3.5;
  const sampleXMax = pts.length > 0 ? Math.max(...pts.map((p) => p.lnStress)) : 4.5;
  const xMin = Math.min(sampleXMin, targetLnStress) - 0.08;
  const xMax = sampleXMax + 0.08;
  const yMin = Math.min(-4.2, rawTargetY - 0.8);
  const yMax = 1.4;

  const sx = (x: number) =>
    padL + ((x - xMin) / Math.max(1e-6, xMax - xMin)) * (w - padL - padR);
  const sy = (y: number) =>
    h - padB - ((y - yMin) / Math.max(1e-6, yMax - yMin)) * (h - padT - padB);

  const yAtXMin = m * (xMin - lnSig0);
  const yAtXMax = m * (xMax - lnSig0);

  const targetY = Math.max(yMin, Math.min(yMax, rawTargetY));

  return (
    <svg
      viewBox={`0 0 ${w} ${h}`}
      style={{
        width: '100%',
        height: '145px',
        background: 'rgba(2, 6, 23, 0.55)',
        borderRadius: '6px',
        border: '1px solid rgba(148, 163, 184, 0.2)',
      }}
    >
      {/* Target P_f horizontal threshold line */}
      <line
        x1={padL}
        y1={sy(targetY)}
        x2={w - padR}
        y2={sy(targetY)}
        stroke="#f59e0b"
        stroke-dasharray="4 3"
        stroke-width="1"
      />
      <text x={padL + 4} y={Math.max(12, sy(targetY) - 4)} fill="#f59e0b" font-size="9">
        Target ln(ln(1/(1−P_f))) = {rawTargetY.toFixed(1)} (P_f = {targetPf.toExponential(0)})
      </text>

      {/* Fitted Weibull regression line */}
      <line
        x1={sx(xMin)}
        y1={sy(yAtXMin)}
        x2={sx(xMax)}
        y2={sy(yAtXMax)}
        stroke="#38bdf8"
        stroke-width="2"
      />

      {/* Empirical coupon sample points */}
      {pts.map((pt, i) => (
        <circle
          key={i}
          cx={sx(pt.lnStress)}
          cy={sy(pt.weibullY)}
          r="3.5"
          fill="#10b981"
          stroke="#022c22"
          stroke-width="1"
        >
          <title>
            {pt.id}: σ₁ = {pt.stressMpa.toFixed(1)} MPa, T = {pt.torqueNm.toFixed(2)} N·m, P_f ={' '}
            {(pt.pf * 100).toFixed(1)}%
          </title>
        </circle>
      ))}

      {/* Axes labels */}
      <text x={w / 2} y={h - 5} fill="#94a3b8" font-size="9.5" text-anchor="middle">
        X = ln(σ₁ [MPa]) · Slope m = {m.toFixed(2)}
      </text>
      <text
        x={11}
        y={h / 2}
        fill="#94a3b8"
        font-size="9"
        text-anchor="middle"
        transform={`rotate(-90, 11, ${h / 2})`}
      >
        ln(−ln(1−P_f))
      </text>
    </svg>
  );
}
