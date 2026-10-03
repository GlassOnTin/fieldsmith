// Command palette (Ctrl K) and pop-up menus.
import { useEffect, useMemo, useRef, useState } from 'preact/hooks';
import { commands } from './commands';
import type { Command } from './commands';
import { addShape } from './commands';
import { SHAPE_TYPES, MODIFIER_LABEL } from '../doc/ops';
import type { Modifier } from '../doc/types';
import { ShapeIcon } from './icons';
import { addModifier } from './Inspector';
import { ui, useApp } from './ui';

function score(c: Command, q: string): number {
  if (!q) return 1;
  const hay = `${c.group} ${c.label}`.toLowerCase();
  const words = q.toLowerCase().split(/\s+/).filter(Boolean);
  let s = 0;
  for (const w of words) {
    const i = hay.indexOf(w);
    if (i < 0) return 0;
    s += i === 0 || hay[i - 1] === ' ' ? 3 : 1;
  }
  return s;
}

export function Palette() {
  useApp();
  const [q, setQ] = useState('');
  const [active, setActive] = useState(0);
  const input = useRef<HTMLInputElement>(null);
  const list = useMemo(() => {
    if (!ui.paletteOpen) return [];
    return commands()
      .map((c) => ({ c, s: score(c, q) }))
      .filter((x) => x.s > 0)
      .sort((a, b) => b.s - a.s)
      .map((x) => x.c);
  }, [q, ui.paletteOpen]);
  useEffect(() => {
    if (ui.paletteOpen) {
      setQ('');
      setActive(0);
      setTimeout(() => input.current?.focus(), 0);
    }
  }, [ui.paletteOpen]);
  if (!ui.paletteOpen) return null;
  const close = () => ui.set('paletteOpen', false);
  const run = (c: Command | undefined) => {
    if (!c || (c.enabled && !c.enabled())) return;
    close();
    c.run();
  };
  return (
    <div class="palette-scrim" onMouseDown={(e) => e.target === e.currentTarget && close()}>
      <div class="palette" role="dialog" aria-label="Command palette">
        <input
          ref={input}
          placeholder="Type a command…"
          value={q}
          data-testid="palette-input"
          onInput={(e) => {
            setQ((e.target as HTMLInputElement).value);
            setActive(0);
          }}
          onKeyDown={(e) => {
            if (e.key === 'Escape') close();
            else if (e.key === 'ArrowDown') {
              e.preventDefault();
              setActive((a) => Math.min(list.length - 1, a + 1));
            } else if (e.key === 'ArrowUp') {
              e.preventDefault();
              setActive((a) => Math.max(0, a - 1));
            } else if (e.key === 'Enter') run(list[active]);
          }}
        />
        {list.length === 0 ? (
          <div class="empty">No matching command.</div>
        ) : (
          <ul>
            {list.map((c, i) => {
              const disabled = c.enabled && !c.enabled();
              return (
                <li key={c.id} class={`${i === active ? 'active' : ''} ${disabled ? 'disabled' : ''}`} onMouseEnter={() => setActive(i)} onClick={() => run(c)} data-command={c.id}>
                  <span class="group">{c.group}</span>
                  <span>{c.label}</span>
                  {c.keys && <span class="kbd">{c.keys}</span>}
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}

export function Menus() {
  useApp();
  const m = ui.menu;
  if (!m) return null;
  const close = () => ui.set('menu', null);
  let items: { label: string; icon?: string; keys?: string; run: () => void; testId?: string }[] = [];
  if (m.kind === 'add') {
    items = SHAPE_TYPES.map((s) => ({ label: s.label, icon: s.type, keys: s.key ? `Shift ${s.key}` : undefined, run: () => void addShape(s.type), testId: `add-${s.type}` }));
  } else if (m.kind === 'modifier' && m.target) {
    items = (Object.keys(MODIFIER_LABEL) as Modifier['type'][]).map((t) => ({ label: MODIFIER_LABEL[t], run: () => addModifier(m.target!, t), testId: `mod-${t}` }));
  }
  return (
    <>
      <div class="scrim" onMouseDown={close} />
      <div class="menu" style={{ left: `${Math.max(8, m.x)}px`, top: `${m.y}px` }} role="menu">
        {items.map((it) => (
          <button
            key={it.label}
            role="menuitem"
            data-testid={it.testId}
            onClick={() => {
              close();
              it.run();
            }}
          >
            {it.icon && <ShapeIcon type={it.icon} />}
            <span>{it.label}</span>
            {it.keys && <span class="kbd">{it.keys}</span>}
          </button>
        ))}
      </div>
    </>
  );
}
