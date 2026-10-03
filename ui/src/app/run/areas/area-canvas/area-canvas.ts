import {
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
import { AreaDraft } from '../area-draft';
import {
  AreaStyle,
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

/** An area held by the pointer: which, its edges (null: moved whole), where the press was, and the area then. */
interface AreaGrip {
  index: number;
  edges: HeldEdges | null;
  start: SharePoint;
  from: AreaRect;
  wasSelected: boolean;
  moved: boolean;
}

/** A new area being drawn: where the drag started and where it is. */
interface AreaSketch {
  start: SharePoint;
  end: SharePoint;
}

/**
 * The excluded areas over the video, to edit with the pointer: a drag on the empty video draws an area, a drag on an
 * area's inside moves it, on its edge or corner resizes it; a click selects one (again: the one under it). Delete
 * removes the selected area, Escape closes the editor.
 */
@Component({
  selector: 'app-area-canvas',
  templateUrl: './area-canvas.html',
  styleUrl: './area-canvas.scss',
  host: { '(document:keydown)': 'handleKeydown($event)' },
})
export class AreaCanvas {
  protected readonly draft = inject(AreaDraft);
  private readonly destroyRef = inject(DestroyRef);
  private readonly canvas = viewChild.required<ElementRef<HTMLCanvasElement>>('areas');
  private readonly grip = signal<AreaGrip | null>(null);
  private readonly sketch = signal<AreaSketch | null>(null);
  private style: AreaStyle | null = null;

  constructor() {
    afterNextRender(() => {
      const resize = new ResizeObserver(() => this.draw());
      resize.observe(this.canvas().nativeElement);
      this.destroyRef.onDestroy(() => resize.disconnect());
    });
    afterRenderEffect(() => {
      this.draft.boxes();
      this.draft.selected();
      this.draft.kinds();
      this.grip();
      this.sketch();
      untracked(() => this.draw());
    });
  }

  private size(): ScreenSize {
    const el = this.canvas().nativeElement;
    return { width: el.clientWidth || 1, height: el.clientHeight || 1 };
  }

  private at(e: PointerEvent): SharePoint {
    const r = this.canvas().nativeElement.getBoundingClientRect();
    return [(e.clientX - r.left) / r.width, (e.clientY - r.top) / r.height];
  }

  private draw(): void {
    const canvas = this.canvas().nativeElement;
    const size = this.size();
    const dpr = devicePixelRatio || 1;
    if (canvas.width !== Math.round(size.width * dpr)) canvas.width = Math.round(size.width * dpr);
    if (canvas.height !== Math.round(size.height * dpr)) {
      canvas.height = Math.round(size.height * dpr);
    }
    const c = canvas.getContext('2d');
    if (!c) return;
    c.setTransform(dpr, 0, 0, dpr, 0, 0);
    c.clearRect(0, 0, size.width, size.height);
    this.style ??= readAreaStyle(canvas);
    const sketch = this.sketch();
    const grip = this.grip();
    const drawing: AreaRect | null = sketch && [
      Math.min(sketch.start[0], sketch.end[0]),
      Math.min(sketch.start[1], sketch.end[1]),
      Math.max(sketch.start[0], sketch.end[0]),
      Math.max(sketch.start[1], sketch.end[1]),
    ];
    drawAreas(
      c,
      {
        boxes: this.draft.boxes(),
        selected: this.draft.selected(),
        drawing,
        moving: grip?.moved ? grip.index : -1,
        kindName: (id) => this.draft.kindName(id),
      },
      size,
      this.style,
    );
  }

  /** The selected area is taken first, then the topmost under the pointer; on the empty video, a new area starts. */
  protected pressArea(e: PointerEvent): void {
    if (e.button !== 0 || !this.draft.ready()) return;
    this.canvas().nativeElement.setPointerCapture(e.pointerId);
    const point = this.at(e);
    const boxes = this.draft.boxes();
    const hold = holdAt(boxes, this.draft.selected(), point, this.size());
    if (!hold) {
      this.sketch.set({ start: point, end: point });
      return;
    }
    const wasSelected = this.draft.selected() === hold.index;
    this.draft.select(hold.index);
    const [a, b, c, d] = boxes[hold.index];
    this.grip.set({
      index: hold.index,
      edges: hold.edges,
      start: point,
      from: [a, b, c, d],
      wasSelected,
      moved: false,
    });
  }

  protected movePointer(e: PointerEvent): void {
    const point = this.at(e);
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
   * A click on the selected area selects the next one under it (areas can overlap); a drawn area is added and
   * selected, so the user can say what it is; a click on the empty video selects nothing.
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
   * Escape closes the editor; Delete or Backspace removes the selected area. Keys typed into a field are its own, and
   * a key the player already used (Escape leaving full screen) is the player's.
   */
  protected handleKeydown(e: KeyboardEvent): void {
    if (e.defaultPrevented || e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.target instanceof Element && e.target.closest('input, textarea, select, dialog')) return;
    if (e.key === 'Escape') {
      this.draft.stop();
    } else if (e.key === 'Delete' || e.key === 'Backspace') {
      e.preventDefault();
      this.draft.remove();
    }
  }
}
