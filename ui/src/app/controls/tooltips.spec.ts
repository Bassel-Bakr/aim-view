import { TestBed } from '@angular/core/testing';
import { Tooltips } from './tooltips';

/** A pointer event of one kind, which jsdom lacks. */
function pointer(
  type: string,
  pointerType: string,
  relatedTarget: EventTarget | null = null,
): Event {
  const event = new MouseEvent(type, { bubbles: true, relatedTarget });
  Object.defineProperty(event, 'pointerType', { value: pointerType });
  return event;
}

describe('Tooltips', () => {
  let button: HTMLButtonElement;
  let shown: boolean;

  beforeEach(() => {
    shown = false;
    // jsdom has no popover API: the spec records what the service asks of it
    HTMLElement.prototype.showPopover = function showPopover() {
      shown = true;
    };
    HTMLElement.prototype.hidePopover = function hidePopover() {
      shown = false;
    };
    button = document.createElement('button');
    button.setAttribute('data-tooltip', 'Saves the areas');
    button.setAttribute('aria-describedby', 'own-note');
    button.innerHTML = '<span>Save</span>';
    document.body.append(button);
    TestBed.inject(Tooltips).start();
  });

  afterEach(() => button.remove());

  it('shows the words for a mouse, names them as a description, and puts the description back', () => {
    button.firstElementChild!.dispatchEvent(pointer('pointerover', 'mouse'));
    const tooltip = document.getElementById('app-tooltip')!;
    expect(shown).toBe(true);
    expect(tooltip.textContent).toBe('Saves the areas');
    expect(tooltip.getAttribute('role')).toBe('tooltip');
    expect(button.getAttribute('aria-describedby')).toBe('own-note app-tooltip');
    // into a child of the part: it stays
    button.dispatchEvent(pointer('pointerout', 'mouse', button.firstElementChild));
    expect(shown).toBe(true);
    button.dispatchEvent(pointer('pointerout', 'mouse', document.body));
    expect(shown).toBe(false);
    expect(button.getAttribute('aria-describedby')).toBe('own-note');
  });

  it('shows nothing for a touch, and hides on Escape and on a press', () => {
    button.dispatchEvent(pointer('pointerover', 'touch'));
    expect(shown).toBe(false);
    button.dispatchEvent(pointer('pointerover', 'mouse'));
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    expect(shown).toBe(false);
    button.dispatchEvent(pointer('pointerover', 'mouse'));
    button.dispatchEvent(pointer('pointerdown', 'mouse'));
    expect(shown).toBe(false);
  });
});
