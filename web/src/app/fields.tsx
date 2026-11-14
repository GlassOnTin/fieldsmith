// Inspector controls: numeric fields with units, arithmetic, arrow-key nudging and label scrubbing.
import { useEffect, useRef, useState } from 'preact/hooks';
import type { ComponentChildren } from 'preact';
import { store } from '../doc/store';
import type { Document } from '../doc/types';
import { exprReferencesLet, removeConsumerExpr, upsertConsumerExpr } from '../doc/param';
import { ui } from './ui';

/** A record the expression table holds. `driven` carries the expression, so the row
 * refuses edits and the hint names what sweeps instead (M: .fsm let-variables). */
const drivenNote = (label: string, expr: string): string =>
  `${label} is written by an expression ('${expr}') — sweep the let that drives it`;

/** Evaluate a small arithmetic expression: numbers, + − * / ( ), unary minus. */
export function evalExpr(src: string): number | null {
  const s = src.replace(/−/g, '-').replace(/,/g, '.').replace(/\s+/g, '');
  if (!s) return null;
  let i = 0;
  const num = (): number | null => {
    if (s[i] === '(') {
      i++;
      const v = add();
      if (s[i] !== ')') return null;
      i++;
      return v;
    }
    if (s[i] === '-') {
      i++;
      const v = num();
      return v === null ? null : -v;
    }
    if (s[i] === '+') {
      i++;
      return num();
    }
    const m = /^(\d+\.?\d*|\.\d+)(e[+-]?\d+)?/i.exec(s.slice(i));
    if (!m) return null;
    i += m[0].length;
    return parseFloat(m[0]);
  };
  const mul = (): number | null => {
    let v = num();
    while (v !== null && (s[i] === '*' || s[i] === '/')) {
      const op = s[i++];
      const r = num();
      if (r === null) return null;
      v = op === '*' ? v * r : v / r;
    }
    return v;
  };
  const add = (): number | null => {
    let v = mul();
    while (v !== null && (s[i] === '+' || s[i] === '-')) {
      const op = s[i++];
      const r = mul();
      if (r === null) return null;
      v = op === '+' ? v + r : v - r;
    }
    return v;
  };
  const v = add();
  return v !== null && i === s.length && Number.isFinite(v) ? v : null;
}

export function fmt(v: number, digits = 4): string {
  if (!Number.isFinite(v)) return '';
  if (v === 0) return '0';
  const abs = Math.abs(v);
  if (abs < 10 ** -digits || abs >= 1e7) {
    return Number(v.toPrecision(4)).toExponential();
  }
  if (abs < 0.01) {
    return String(Number(v.toPrecision(4)));
  }
  const r = Math.round(v * 10 ** digits) / 10 ** digits;
  return String(Object.is(r, -0) ? 0 : r);
}

interface NumProps {
  value: number;
  unit?: string;
  min?: number;
  max?: number;
  step?: number;
  axis?: 'x' | 'y' | 'z';
  integer?: boolean;
  /** Called with the committed value. `live` = during a scrub/drag (transient). */
  onChange: (v: number, live: boolean) => void;
  onCommit?: () => void;
  /** Optional `.fsm` `let`-expression binder. Returns evaluated display value on success, or `null` on error. */
  onCommitExpr?: (rawExpr: string) => number | null;
  onClearExpr?: () => void;
  driven?: string | null;
  label?: string;
  testId?: string;
  title?: string;
}

function clampV(v: number, p: NumProps) {
  if (p.min !== undefined) v = Math.max(p.min, v);
  if (p.max !== undefined) v = Math.min(p.max, v);
  if (p.integer) v = Math.round(v);
  return v;
}

export function NumberInput(p: NumProps) {
  const [text, setText] = useState(fmt(p.value));
  const [focused, setFocused] = useState(false);
  const [invalid, setInvalid] = useState(false);
  useEffect(() => {
    if (!focused) setText(fmt(p.value));
  }, [p.value, focused]);
  const commit = () => {
    const trimmed = text.trim();
    if (p.onCommitExpr && (trimmed.startsWith('=') || exprReferencesLet(store.doc, trimmed))) {
      const ev = p.onCommitExpr(trimmed);
      if (ev === null) {
        setInvalid(true);
        return;
      }
      setInvalid(false);
      setText(fmt(ev));
      return;
    }
    const v = evalExpr(text);
    if (v === null) {
      if (p.onCommitExpr && /[a-zA-Z_]/.test(trimmed)) {
        const ev = p.onCommitExpr(trimmed);
        if (ev !== null) {
          setInvalid(false);
          setText(fmt(ev));
          return;
        }
      }
      setInvalid(true);
      return;
    }
    setInvalid(false);
    const c = clampV(v, p);
    if (c !== p.value) p.onChange(c, false);
    setText(fmt(c));
  };
  return (
    <div class={`numbox ${p.axis ? 'has-axis' : ''} ${invalid ? 'invalid' : ''}`}>
      {p.axis && <span class={`axis ${p.axis}`}>{p.axis.toUpperCase()}</span>}
      <input
        class={`num ${invalid ? 'invalid' : ''}`}
        value={text}
        aria-label={p.label}
        data-testid={p.testId}
        title={p.title}
        spellcheck={false}
        onFocus={(e) => {
          setFocused(true);
          (e.target as HTMLInputElement).select();
        }}
        onBlur={() => {
          setFocused(false);
          commit();
        }}
        onInput={(e) => {
          setText((e.target as HTMLInputElement).value);
          setInvalid(false);
        }}
        onKeyDown={(e) => {
          e.stopPropagation();
          if (e.key === 'Enter') {
            commit();
            (e.target as HTMLInputElement).blur();
          } else if (e.key === 'Escape') {
            setText(fmt(p.value));
            setInvalid(false);
            (e.target as HTMLInputElement).blur();
          } else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
            e.preventDefault();
            const base = evalExpr(text) ?? p.value;
            const st = (p.step ?? 1) * (e.shiftKey ? 10 : e.altKey ? 0.1 : 1);
            const c = clampV(base + (e.key === 'ArrowUp' ? st : -st), p);
            setText(fmt(c));
            p.onChange(c, false);
          }
        }}
      />
      {p.driven && (
        <button
          type="button"
          class="fx-chip"
          data-testid={p.testId ? `${p.testId}-fx` : undefined}
          title={
            p.onClearExpr
              ? `Driven by '${p.driven}' — click to unlink expression`
              : `Driven by '${p.driven}'`
          }
          onMouseDown={(e) => e.preventDefault()}
          onClick={(e) => {
            e.stopPropagation();
            p.onClearExpr?.();
          }}
        >
          fx
        </button>
      )}
      {p.unit && <span class="unit">{p.unit}</span>}
    </div>
  );
}

/** A label that scrubs a value when dragged horizontally (Shift = ×10, Alt = ×0.1). */
export function ScrubLabel(props: { children: ComponentChildren; value: number; step?: number; min?: number; max?: number; integer?: boolean; onChange: (v: number, live: boolean) => void; title?: string }) {
  const ref = useRef<HTMLLabelElement>(null);
  return (
    <label
      ref={ref}
      title={props.title ?? 'Drag to adjust'}
      onPointerDown={(e) => {
        const el = ref.current!;
        el.setPointerCapture(e.pointerId);
        const x0 = e.clientX;
        const v0 = props.value;
        let moved = false;
        const move = (ev: PointerEvent) => {
          const dx = ev.clientX - x0;
          if (Math.abs(dx) < 2 && !moved) return;
          moved = true;
          const st = (props.step ?? 1) * (ev.shiftKey ? 10 : ev.altKey ? 0.1 : 1);
          let v = v0 + Math.round(dx / 4) * st;
          if (props.min !== undefined) v = Math.max(props.min, v);
          if (props.max !== undefined) v = Math.min(props.max, v);
          if (props.integer) v = Math.round(v);
          props.onChange(Math.round(v * 1e6) / 1e6, true);
        };
        const up = () => {
          el.removeEventListener('pointermove', move);
          el.removeEventListener('pointerup', up);
          if (moved) store.commitTransient('Adjust value');
        };
        el.addEventListener('pointermove', move);
        el.addEventListener('pointerup', up);
      }}
    >
      {props.children}
    </label>
  );
}

/**
 * One labelled numeric row bound to the document. `get`/`set` address the value in a draft;
 * edits are undoable, scrubs are transient until release.
 */
export function NumRow(props: {
  label: string;
  unit?: string;
  get: (d: Document) => number;
  set: (d: Document, v: number) => void;
  min?: number;
  max?: number;
  step?: number;
  integer?: boolean;
  undoLabel: string;
  testId?: string;
  title?: string;
  driven?: string | null;
  exprTarget?: { target: string; attr: string; comp?: number | null; scale?: number };
}) {
  const value = props.get(store.doc);
  const apply = (v: number, live: boolean) => {
    if (props.driven) {
      // An expression writes this number: editing it here would be overwritten at the
      // next let sweep. The sweep panel edits the let that drives it.
      ui.notify(drivenNote(props.label, props.driven), 'error');
      return;
    }
    if (live) store.transient((d) => props.set(d, v));
    else store.transact(props.undoLabel, (d) => props.set(d, v));
  };
  const commitExpr = props.exprTarget
    ? (raw: string): number | null => {
        const et = props.exprTarget!;
        let outVal: number | null = null;
        let errMsg: string | null = null;
        try {
          store.transact(`Bind ${props.label} expression`, (d) => {
            const rawVal = upsertConsumerExpr(d, et.target, et.attr, et.comp ?? null, raw, et.scale ?? 1);
            let clamped = rawVal;
            if (props.min !== undefined && clamped < props.min) {
              throw new Error(`${props.label} must be ≥ ${props.min}`);
            }
            if (props.max !== undefined && clamped > props.max) {
              throw new Error(`${props.label} must be ≤ ${props.max}`);
            }
            if (props.integer) clamped = Math.round(clamped);
            props.set(d, clamped);
            outVal = clamped;
          });
        } catch (e) {
          errMsg = e instanceof Error ? e.message : String(e);
        }
        if (errMsg) {
          ui.notify(`${props.label}: ${errMsg}`, 'error');
          return null;
        }
        return outVal;
      }
    : undefined;
  const clearExpr =
    props.exprTarget && props.driven
      ? () => {
          const et = props.exprTarget!;
          const oldExpr = props.driven!;
          store.transact(`Unlink ${props.label} expression`, (d) => {
            removeConsumerExpr(d, et.target, et.attr, et.comp ?? null);
          });
          ui.notify(`Unlinked '${oldExpr}' from ${props.label}`);
        }
      : undefined;
  return (
    <>
      <ScrubLabel value={value} step={props.step} min={props.min} max={props.max} integer={props.integer} onChange={apply} title={props.driven ? drivenNote(props.label, props.driven) : props.title}>
        {props.label}
      </ScrubLabel>
      <NumberInput
        value={value}
        unit={props.unit}
        min={props.min}
        max={props.max}
        step={props.step}
        integer={props.integer}
        onChange={apply}
        onCommitExpr={commitExpr}
        onClearExpr={clearExpr}
        driven={props.driven}
        label={props.label}
        testId={props.testId}
        title={props.driven ? drivenNote(props.label, props.driven) : undefined}
      />
    </>
  );
}

/** Three numeric inputs for a vector. `driven(i)` returns the driving expression for
 * component i (or null), so each axis refuses on its own. */
export function VecRow(props: {
  label: string;
  unit?: string;
  get: (d: Document) => [number, number, number];
  set: (d: Document, v: [number, number, number]) => void;
  step?: number;
  min?: number;
  undoLabel: string;
  testId?: string;
  driven?: (i: number) => string | null;
  exprTarget?: { target: string; attr: string };
}) {
  const v = props.get(store.doc);
  const axes = ['x', 'y', 'z'] as const;
  const note = (i: number) => props.driven?.(i) ?? null;
  return (
    <>
      <label class="plain">{props.unit ? `${props.label} (${props.unit})` : props.label}</label>
      <div class="vec">
        {axes.map((a, i) => (
          <NumberInput
            key={a}
            axis={a}
            value={v[i]}
            step={props.step}
            min={props.min}
            label={`${props.label} ${a}`}
            testId={props.testId ? `${props.testId}-${a}` : undefined}
            driven={note(i)}
            title={note(i) ? drivenNote(`${props.label} ${a}`, note(i)!) : undefined}
            onCommitExpr={
              props.exprTarget
                ? (raw): number | null => {
                    const et = props.exprTarget!;
                    let outVal: number | null = null;
                    let errMsg: string | null = null;
                    try {
                      store.transact(`Bind ${props.label} ${a} expression`, (d) => {
                        const rawVal = upsertConsumerExpr(d, et.target, et.attr, i, raw, 1);
                        if (props.min !== undefined && rawVal < props.min) {
                          throw new Error(`${props.label} ${a} must be ≥ ${props.min}`);
                        }
                        const cur = [...props.get(d)] as [number, number, number];
                        cur[i] = rawVal;
                        props.set(d, cur);
                        outVal = rawVal;
                      });
                    } catch (e) {
                      errMsg = e instanceof Error ? e.message : String(e);
                    }
                    if (errMsg) {
                      ui.notify(`${props.label} ${a}: ${errMsg}`, 'error');
                      return null;
                    }
                    return outVal;
                  }
                : undefined
            }
            onClearExpr={
              props.exprTarget && note(i)
                ? () => {
                    const et = props.exprTarget!;
                    const oldExpr = note(i)!;
                    store.transact(`Unlink ${props.label} ${a} expression`, (d) => {
                      removeConsumerExpr(d, et.target, et.attr, i);
                    });
                    ui.notify(`Unlinked '${oldExpr}' from ${props.label} ${a}`);
                  }
                : undefined
            }
            onChange={(x, live) => {
              if (note(i)) {
                ui.notify(drivenNote(`${props.label} ${a}`, note(i)!), 'error');
                return;
              }
              const f = (d: Document) => {
                const cur = [...props.get(d)] as [number, number, number];
                cur[i] = x;
                props.set(d, cur);
              };
              if (live) store.transient(f);
              else store.transact(props.undoLabel, f);
            }}
          />
        ))}
      </div>
    </>
  );
}

export function Seg<T extends string>(props: { value: T; options: { value: T; label: string; title?: string }[]; onChange: (v: T) => void; testId?: string }) {
  return (
    <div class="seg" data-testid={props.testId}>
      {props.options.map((o) => (
        <button key={o.value} class={o.value === props.value ? 'on' : ''} title={o.title} data-value={o.value} onClick={() => props.onChange(o.value)}>
          {o.label}
        </button>
      ))}
    </div>
  );
}
