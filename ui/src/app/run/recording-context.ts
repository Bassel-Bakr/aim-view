/**
 * A stand-in for a canvas's 2D context in specs. jsdom has no canvas to draw on, so a drawing's
 * spec pins what the drawing asks of the context instead: every call and every property set, in
 * order, with their values. The same calls draw the same pixels. Text measures a fixed width a
 * character, so a layout that depends on text widths is pinned too. In: a drawing's calls. Out: the
 * specs of the run page's drawings and chart models, which compare the calls or their fingerprint.
 */

/** A value a drawing passes to the context, as recorded. */
export type CanvasValue = string | number | boolean | null | CanvasValue[];

/**
 * One thing a drawing did: a method's name and its arguments, or `set <property>` and the value.
 */
export type CanvasCall = [name: string, ...values: CanvasValue[]];

/** The context to draw on, and what was drawn on it. */
export interface RecordingContext {
  /** The stand-in context the drawing draws on. */
  context: CanvasRenderingContext2D;
  /** Every call and property set the drawing made, in order. */
  calls: CanvasCall[];
  /** A short fingerprint of every call, to pin a long drawing in a spec. */
  digest(): string;
}

/** How wide each character of a text measures, in pixels. */
const CHARACTER_WIDTH_PX = 7;
/** The context's own starting state, for a drawing that reads a property before it sets it. */
const START_STATE: Record<string, unknown> = {
  font: '10px sans-serif',
  fillStyle: '#000000',
  strokeStyle: '#000000',
  lineWidth: 1,
  globalAlpha: 1,
  textAlign: 'start',
  textBaseline: 'alphabetic',
  lineCap: 'butt',
  lineJoin: 'miter',
};
/** FNV-1a's 32-bit offset basis, the hash's starting value. */
const FNV_OFFSET = 0x811c9dc5;
/** FNV-1a's 32-bit prime, which each character's step multiplies by. */
const FNV_PRIME = 0x01000193;

/**
 * A value as the record keeps it: strings, numbers, booleans, null and arrays of them as they are;
 * anything else (a gradient, a path) as its type in brackets, such as "[object]".
 */
function recorded(value: unknown): CanvasValue {
  if (Array.isArray(value)) return value.map(recorded);
  if (value === null || ['string', 'number', 'boolean'].includes(typeof value)) {
    return value as CanvasValue;
  }
  return `[${typeof value}]`;
}

/**
 * A short fingerprint of a text (FNV-1a, as eight hex digits), to pin a long output in a spec: a
 * drawing's calls, or a chart model as JSON.
 */
export function fingerprint(text: string): string {
  let hash = FNV_OFFSET;
  for (let i = 0; i < text.length; i++) {
    hash ^= text.charCodeAt(i);
    hash = Math.imul(hash, FNV_PRIME) >>> 0;
  }
  return hash.toString(16).padStart(8, '0');
}

/**
 * A context on a canvas of this size in pixels (its client size the same) that records every call
 * and property set. Properties read back what was set, or the context's starting state; any method
 * not named here records its call and does nothing.
 */
export function recordingContext(widthPx = 1280, heightPx = 720): RecordingContext {
  const calls: CanvasCall[] = [];
  const state = new Map<string, unknown>(Object.entries(START_STATE));
  const canvas = { width: widthPx, height: heightPx, clientWidth: widthPx, clientHeight: heightPx };
  let lineDash: number[] = [];
  const methods: Record<string, (...values: unknown[]) => unknown> = {
    measureText: (text) => {
      calls.push(['measureText', recorded(text)]);
      return { width: String(text).length * CHARACTER_WIDTH_PX };
    },
    setLineDash: (dash) => {
      calls.push(['setLineDash', recorded(dash)]);
      lineDash = [...(dash as number[])];
    },
    getLineDash: () => [...lineDash],
  };
  const context = new Proxy(
    {},
    {
      get: (_target, property) => {
        const name = String(property);
        if (name === 'canvas') return canvas;
        if (methods[name]) return methods[name];
        if (state.has(name)) return state.get(name);
        return (...values: unknown[]) => {
          calls.push([name, ...values.map(recorded)]);
        };
      },
      set: (_target, property, value) => {
        const name = String(property);
        calls.push([`set ${name}`, recorded(value)]);
        state.set(name, value);
        return true;
      },
    },
  ) as CanvasRenderingContext2D;
  return { context, calls, digest: () => fingerprint(JSON.stringify(calls)) };
}
