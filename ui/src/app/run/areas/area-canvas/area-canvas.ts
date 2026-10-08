/**
 * The excluded areas editor's canvas over the video. In: the editor's areas, selection and kinds
 * (AreaDraft) and the pointer and keys. Out: the areas drawn on the canvas, and the edits to
 * AreaDraft: add, place, select, remove, close.
 */

import {
  computed,
  afterNextRender,
  afterRenderEffect,
  Component,
  DestroyRef,
  ElementRef,
  inject,
  signal,
  untracked,
  viewChild,
} from '@angular/core';
import { AreaRect } from '../../../api';
import { listenQuietly } from '../../../services/listen-quietly';
import { AreaDraft } from '../area-draft';
import {
  cursorFor,
  DRAG_PX,
  dragArea,
  drawAreas,
  drawnArea,
  HeldEdges,
  holdAt,
  nextUnder,
  readAreaStyle,
  ScreenSize,
  SharePoint,
} from '../area-geometry';
import { Theme } from '../../../services/theme';

/**
 * An area held by the pointer: which, its edges (null: moved whole), where the press was, and the
 * area then.
 */
interface AreaGrip {
  /** The held area's index. */
  index: number;
  /** The edges held, or null when the area moves whole. */
  edges: HeldEdges | null;
  /** Where the press was, as shares of the video. */
  start: SharePoint;
  /** The area's place at the press, as shares of the frame. */
  from: AreaRect;
  /** The area was already selected at the press, so a click selects the one under it. */
  wasSelected: boolean;
  /** The pointer went DRAG_PX or more, so the press is a drag, not a click. */
  moved: boolean;
}

/** A new area being drawn: where the drag started and where it is. */
interface AreaSketch {
  /** Where the drag started, as shares of the video. */
  start: SharePoint;
  /** Where the pointer is now, as shares of the video. */
  end: SharePoint;
}

/**
 * The excluded areas over the video, to edit with the pointer: a drag on the empty video draws an
 * area, a drag on an area's inside moves it, on its edge or corner resizes it; a click selects one
 * (again: the one under it). Delete removes the selected area, Escape closes the editor.
 */
@Component({
  selector: 'app-area-canvas',
  templateUrl: './area-canvas.html',
  styleUrl: './area-canvas.scss',
  host: { '(document:keydown)': 'handleKeydown($event)' },
})
export class AreaCanvas {
  /** The editor's state, which the canvas draws and changes. */
  protected readonly draft = inject(AreaDraft);
  /** Ends the resize observer and the pointer listener with the view. */
  private readonly destroyRef = inject(DestroyRef);
  /** The canvas the areas are drawn on, over the video. */
  private readonly canvas = viewChild.required<ElementRef<HTMLCanvasElement>>('areas');
  /** The area the pointer holds, or null. */
  private readonly grip = signal<AreaGrip | null>(null);
  /** The area being drawn, or null. */
  private readonly sketch = signal<AreaSketch | null>(null);
  /** The color scheme: the colors below are read again when it changes. */
  private readonly theme = inject(Theme);
  /** The areas' drawing style, read from the tokens on the first draw. */
  private readonly style = computed(() => {
    this.theme.scheme();
    return readAreaStyle(this.canvas().nativeElement);
  });

  /**
   * Draws again when the canvas is resized or the areas, selection, kinds or pointer change, and
   * follows the pointer outside the template (it moves many times a second).
   */
  constructor() {
    afterNextRender(() => {
      const canvas = this.canvas().nativeElement;
      const resize = new ResizeObserver(() => this.draw());
      resize.observe(canvas);
      this.destroyRef.onDestroy(() => resize.disconnect());
      listenQuietly(canvas, 'pointermove', (event) => this.movePointer(event), this.destroyRef);
    });
    afterRenderEffect(() => {
      this.draft.boxes();
      this.draft.selected();
      this.draft.kinds();
      this.grip();
      this.sketch();
      this.style();
      untracked(() => this.draw());
    });
  }

  /**
   * The canvas's size on screen in CSS pixels (at least 1 each way, so shares never divide by 0).
   */
  private size(): ScreenSize {
    const canvas = this.canvas().nativeElement;
    return { width: canvas.clientWidth || 1, height: canvas.clientHeight || 1 };
  }

  /** The pointer's place as shares of the canvas's width and height. */
  private at(event: PointerEvent): SharePoint {
    const bounds = this.canvas().nativeElement.getBoundingClientRect();
    return [
      (event.clientX - bounds.left) / bounds.width,
      (event.clientY - bounds.top) / bounds.height,
    ];
  }

  /**
   * Draws the areas and the one being drawn, at the screen's pixel ratio; sizes the canvas's pixels
   * to its size on screen first.
   */
  private draw(): void {
    const canvas = this.canvas().nativeElement;
    const size = this.size();
    const pixelRatio = devicePixelRatio || 1;
    if (canvas.width !== Math.round(size.width * pixelRatio))
      canvas.width = Math.round(size.width * pixelRatio);
    if (canvas.height !== Math.round(size.height * pixelRatio)) {
      canvas.height = Math.round(size.height * pixelRatio);
    }
    const context = canvas.getContext('2d');
    if (!context) return;
    context.setTransform(pixelRatio, 0, 0, pixelRatio, 0, 0);
    context.clearRect(0, 0, size.width, size.height);
    const sketch = this.sketch();
    const grip = this.grip();
    const drawing: AreaRect | null = sketch && [
      Math.min(sketch.start[0], sketch.end[0]),
      Math.min(sketch.start[1], sketch.end[1]),
      Math.max(sketch.start[0], sketch.end[0]),
      Math.max(sketch.start[1], sketch.end[1]),
    ];
    drawAreas(
      context,
      {
        boxes: this.draft.boxes(),
        selected: this.draft.selected(),
        drawing,
        moving: grip?.moved ? grip.index : -1,
        kindName: (id) => this.draft.kindName(id),
      },
      size,
      this.style(),
    );
  }

  /**
   * A press with the main button: the selected area is taken first, then the topmost under the
   * pointer, and selected; on the empty video, a new area starts. Nothing before the areas are
   * read.
   */
  protected pressArea(event: PointerEvent): void {
    if (event.button !== 0 || !this.draft.ready()) return;
    this.canvas().nativeElement.setPointerCapture(event.pointerId);
    const point = this.at(event);
    const boxes = this.draft.boxes();
    const hold = holdAt(boxes, this.draft.selected(), point, this.size());
    if (!hold) {
      this.sketch.set({ start: point, end: point });
      return;
    }
    const wasSelected = this.draft.selected() === hold.index;
    this.draft.select(hold.index);
    const [left, top, right, bottom] = boxes[hold.index];
    this.grip.set({
      index: hold.index,
      edges: hold.edges,
      start: point,
      from: [left, top, right, bottom],
      wasSelected,
      moved: false,
    });
  }

  /**
   * The pointer moved: a held area moves or resizes once the pointer has gone DRAG_PX, a drawn area
   * follows it, and otherwise the cursor shows what a press would do.
   */
  private movePointer(event: PointerEvent): void {
    const point = this.at(event);
    const size = this.size();
    const grip = this.grip();
    if (grip) {
      const delta: SharePoint = [point[0] - grip.start[0], point[1] - grip.start[1]];
      const far =
        Math.abs(delta[0]) * size.width >= DRAG_PX || Math.abs(delta[1]) * size.height >= DRAG_PX;
      if (!grip.moved && !far) return;
      if (!grip.moved) this.grip.set({ ...grip, moved: true });
      this.draft.place(grip.index, dragArea(grip.from, grip.edges, delta, size));
      return;
    }
    const sketch = this.sketch();
    if (sketch) {
      this.sketch.set({ ...sketch, end: point });
      return;
    }
    const hold = this.draft.ready()
      ? holdAt(this.draft.boxes(), this.draft.selected(), point, size)
      : null;
    this.canvas().nativeElement.style.cursor = cursorFor(hold);
  }

  /**
   * A click on the selected area selects the next one under it (areas can overlap); a drawn area is
   * added and selected, so the user can say what it is; a click on the empty video selects nothing.
   */
  protected releasePointer(): void {
    const grip = this.grip();
    if (grip) {
      this.grip.set(null);
      if (!grip.moved && grip.wasSelected) {
        const under = nextUnder(this.draft.boxes(), grip.index, grip.start);
        if (under >= 0) this.draft.select(under);
      }
      return;
    }
    const sketch = this.sketch();
    if (!sketch) return;
    this.sketch.set(null);
    const rect = drawnArea(sketch.start, sketch.end, this.size());
    if (rect) this.draft.add(rect);
    else this.draft.select(-1);
  }

  /**
   * Escape closes the editor; Delete or Backspace removes the selected area. Keys typed into a
   * field are its own, and a key the player already used (Escape leaving full screen) is the
   * player's.
   */
  protected handleKeydown(event: KeyboardEvent): void {
    if (event.defaultPrevented || event.ctrlKey || event.metaKey || event.altKey) return;
    if (event.target instanceof Element && event.target.closest('input, textarea, select, dialog'))
      return;
    if (event.key === 'Escape') {
      this.draft.stop();
    } else if (event.key === 'Delete' || event.key === 'Backspace') {
      event.preventDefault();
      this.draft.remove();
    }
  }
}
