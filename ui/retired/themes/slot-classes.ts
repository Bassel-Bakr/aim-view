/** A component's slots as class strings. */
export type SlotClasses<S> = { [K in keyof S]: string };

/**
 * A tv() component's slots (with no variants chosen) as plain class strings, built once. Templates then bind a
 * property, not a function call that would run again on every change detection.
 */
export function slotClasses<S extends object>(slots: S): SlotClasses<S> {
  const out: Partial<SlotClasses<S>> = {};
  for (const [key, slot] of Object.entries(slots)) out[key as keyof S] = (slot as () => string)();
  return out as SlotClasses<S>;
}
