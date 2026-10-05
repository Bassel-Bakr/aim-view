import {
  afterNextRender,
  afterRenderEffect,
  Component,
  DestroyRef,
  effect,
  ElementRef,
  inject,
  resource,
  signal,
  untracked,
  viewChild,
} from '@angular/core';
import { CropEntry, SceneView, Shape } from '../../api';
import { CropSets } from '../../platform/crop-sets';
import { CropPoint } from '../../shapes/shape-geometry';
import { CROP_SIDE, CropDraft } from '../crop-draft';
import { DraftScene, removed, uncrossed, withDrawn, withPoint } from '../crop-scene';
import { CropGrip, dragged, gripAt, sketchBox } from './crop-grip';
import {
  CropStyle,
  maskPicture,
  paintStage,
  readCropStyle,
  StagePicture,
  StagePlace,
} from './crop-paint';

/** A point on the stage, in CSS pixels from its top left. */
type ScreenPoint = [x: number, y: number];

/** The zoom and pan: how many times the crop is enlarged, and where its top left is (CSS pixels). */
interface StageZoom {
  zoom: number;
  left: number;
  top: number;
}

/** One finger's press: what it holds (null outside a fix: peek or pan), where it began, and the scene and zoom then. */
interface StagePress {
  grip: CropGrip | null;
  screen: ScreenPoint;
  start: CropPoint;
  from: DraftScene | null;
  zoom: StageZoom;
  moved: boolean;
}

/** Two fingers: how far apart and where their middle was when the second came down, and the zoom then. */
interface StagePinch {
  distance: number;
  middle: ScreenPoint;
  zoom: StageZoom;
}

/** Which crop's picture to load. */
interface CropImageKey {
  folder: string;
  id: string;
}

/** The tinted mask made for a scene view. */
interface StageMask {
  view: SceneView;
  picture: HTMLCanvasElement;
}

const NO_ZOOM: StageZoom = { zoom: 1, left: 0, top: 0 };
const MAX_ZOOM = 8;
/** One wheel step zooms by this much. */
const WHEEL_STEP = 1.25;
/** A finger moving this far (CSS pixels) drags; less is a tap. */
const DRAG_PX = 6;
/** A finger held this long (ms) without moving shows the crop without its marks. */
const PEEK_MS = 180;

/**
 * The crop on show and its shapes, on a canvas. Outside a fix, a held finger peeks under the marks and a moving one
 * pans a zoomed crop. In a fix: a drag on the wall draws a shape of the chosen kind, a drag on a shape moves it (with
 * the selection), and a shape selected alone shows handles: its corners resize it, its handle above turns it and a
 * box's square handle pulls out its third face. A tap selects or unselects a shape, brings a crossed-out box back, and
 * on the wall clears the selection or, with none, marks a tiny target. Two fingers (or the wheel) zoom and pan.
 * Delete removes the selected shapes.
 */
@Component({
  selector: 'app-crop-stage',
  templateUrl: './crop-stage.html',
  styleUrl: './crop-stage.scss',
  host: { '(document:keydown)': 'handleKeydown($event)' },
})
export class CropStage {
  private readonly draft = inject(CropDraft);
  private readonly sets = inject(CropSets);
  private readonly destroyRef = inject(DestroyRef);
  private readonly canvas = viewChild.required<ElementRef<HTMLCanvasElement>>('stage');
  private readonly zoom = signal<StageZoom>(NO_ZOOM);
  private readonly sketch = signal<Shape | null>(null);
  private readonly peeking = signal(false);
  private readonly pointers = new Map<number, ScreenPoint>();
  private press: StagePress | null = null;
  private pinch: StagePinch | null = null;
  private peekTimer: ReturnType<typeof setTimeout> | undefined;
  private style: CropStyle | null = null;
  private lastView: SceneView | null = null;
  private mask: StageMask | null = null;
  private readonly image = resource<ImageBitmap, CropImageKey | undefined>({
    params: () => {
      const [folder, crop] = [this.draft.folder(), this.draft.crop()];
      return folder && crop ? { folder, id: crop.id } : undefined;
    },
    loader: async ({ params }) =>
      createImageBitmap(await this.sets.image(params.folder, params.id)),
  });

  constructor() {
    afterNextRender(() => {
      const resize = new ResizeObserver(() => this.draw());
      resize.observe(this.canvas().nativeElement);
      this.destroyRef.onDestroy(() => resize.disconnect());
    });
    effect(() => {
      this.draft.crop();
      untracked(() => this.zoom.set(NO_ZOOM));
    });
    afterRenderEffect(() => {
      this.draft.shown();
      this.draft.selection();
      this.draft.mode();
      if (this.draft.view.hasValue()) this.draft.view.value();
      if (this.image.hasValue()) this.image.value();
      this.zoom();
      this.sketch();
      this.peeking();
      untracked(() => this.draw());
    });
  }

  private draw(): void {
    const canvas = this.canvas().nativeElement;
    const side = canvas.clientWidth || 1;
    const ratio = devicePixelRatio || 1;
    const pixels = Math.round(side * ratio);
    if (canvas.width !== pixels) [canvas.width, canvas.height] = [pixels, pixels];
    const context = canvas.getContext('2d');
    if (!context) return;
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.clearRect(0, 0, side, side);
    const crop = this.draft.crop();
    if (!crop) return;
    this.style ??= readCropStyle(canvas);
    paintStage(context, this.picture(crop, this.style), this.place(), this.style);
  }

  /** What to draw: the core's last view stands while the next one is worked out, so the mask never blinks. */
  private picture(crop: CropEntry, style: CropStyle): StagePicture {
    const view = this.draft.view.hasValue() ? this.draft.view.value() : this.lastView;
    this.lastView = view;
    if (view && this.mask?.view !== view)
      this.mask = { view, picture: maskPicture(view.mask, style.visible) };
    return {
      image: this.image.hasValue() ? this.image.value() : null,
      crop,
      scene: this.peeking() ? null : this.draft.shown(),
      view,
      mask: view && this.mask ? this.mask.picture : null,
      selection: this.draft.selection(),
      editing: this.draft.mode() === 'fix',
      sketch: this.sketch(),
    };
  }

  private place(): StagePlace {
    const side = this.canvas().nativeElement.clientWidth || 1;
    const { zoom, left, top } = this.zoom();
    return { scale: (side / CROP_SIDE) * zoom, left, top };
  }

  private at(event: MouseEvent): ScreenPoint {
    const bounds = this.canvas().nativeElement.getBoundingClientRect();
    return [event.clientX - bounds.left, event.clientY - bounds.top];
  }

  private toCrop([x, y]: ScreenPoint): CropPoint {
    const { scale, left, top } = this.place();
    const keep = (value: number) => Math.min(Math.max(value, 0), CROP_SIDE);
    return [keep((x - left) / scale), keep((y - top) / scale)];
  }

  /** A zoom with the crop kept over the whole stage. */
  private clamped({ zoom, left, top }: StageZoom): StageZoom {
    const side = this.canvas().nativeElement.clientWidth || 1;
    const keep = (value: number) => Math.min(Math.max(value, side - side * zoom), 0);
    return { zoom, left: keep(left), top: keep(top) };
  }

  protected pressStage(event: PointerEvent): void {
    if (event.pointerType === 'mouse' && event.button !== 0) return;
    this.canvas().nativeElement.setPointerCapture(event.pointerId);
    const screen = this.at(event);
    this.pointers.set(event.pointerId, screen);
    if (this.pointers.size === 2) {
      this.startPinch();
      return;
    }
    if (this.pointers.size > 2) return;
    const start = this.toCrop(screen);
    const [scene, crop] = [
      this.draft.mode() === 'fix' ? this.draft.draft() : null,
      this.draft.crop(),
    ];
    const grip =
      scene && crop ? gripAt(scene, crop, this.draft.selection(), start, this.place().scale) : null;
    this.press = { grip, screen, start, from: scene, zoom: this.zoom(), moved: false };
    if (!grip)
      this.peekTimer = setTimeout(() => this.peeking.set(this.press?.moved === false), PEEK_MS);
  }

  /** A second finger: whatever the first was doing is undone, and the two zoom and pan. */
  private startPinch(): void {
    this.cancelPress();
    const [a, b] = [...this.pointers.values()];
    this.pinch = {
      distance: Math.hypot(a[0] - b[0], a[1] - b[1]) || 1,
      middle: [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2],
      zoom: this.zoom(),
    };
  }

  private cancelPress(): void {
    clearTimeout(this.peekTimer);
    this.peeking.set(false);
    this.sketch.set(null);
    const press = this.press;
    this.press = null;
    const from = press?.from;
    if (from && press.moved) this.draft.edit(() => from);
  }

  protected movePointer(event: PointerEvent): void {
    if (!this.pointers.has(event.pointerId)) return;
    const screen = this.at(event);
    this.pointers.set(event.pointerId, screen);
    if (this.pinch) {
      this.movePinch(this.pinch);
      return;
    }
    const press = this.press;
    if (!press) return;
    if (
      !press.moved &&
      Math.hypot(screen[0] - press.screen[0], screen[1] - press.screen[1]) < DRAG_PX
    )
      return;
    if (!press.moved) {
      press.moved = true;
      clearTimeout(this.peekTimer);
      this.peeking.set(false);
    }
    this.dragTo(press, screen);
  }

  private movePinch(pinch: StagePinch): void {
    if (this.pointers.size < 2) return;
    const [a, b] = [...this.pointers.values()];
    const middle: ScreenPoint = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2];
    const from = pinch.zoom;
    const spread = Math.hypot(a[0] - b[0], a[1] - b[1]) / pinch.distance;
    const zoom = Math.min(Math.max(from.zoom * spread, 1), MAX_ZOOM);
    const grow = zoom / from.zoom;
    this.zoom.set(
      this.clamped({
        zoom,
        left: middle[0] - (pinch.middle[0] - from.left) * grow,
        top: middle[1] - (pinch.middle[1] - from.top) * grow,
      }),
    );
  }

  private dragTo(press: StagePress, screen: ScreenPoint): void {
    const grip = press.grip;
    if (!grip) {
      const [dx, dy] = [screen[0] - press.screen[0], screen[1] - press.screen[1]];
      this.zoom.set(
        this.clamped({ ...press.zoom, left: press.zoom.left + dx, top: press.zoom.top + dy }),
      );
      return;
    }
    const point = this.toCrop(screen);
    if (grip.kind === 'draw') {
      const box = sketchBox(press.start, point);
      const kind = this.draft.kind();
      this.sketch.set(
        box && { id: '', kind, box, angle: 0, face: null, depth: 0, role: null, model: null },
      );
      return;
    }
    const from = press.from;
    if (from)
      this.draft.edit(() => dragged(from, grip, this.draft.selection(), press.start, point));
  }

  protected releasePointer(event: PointerEvent): void {
    if (!this.pointers.delete(event.pointerId)) return;
    if (this.pinch) {
      if (!this.pointers.size) this.pinch = null;
      return;
    }
    const press = this.press;
    if (!press) return;
    if (event.type === 'pointercancel') {
      this.cancelPress();
      return;
    }
    this.press = null;
    clearTimeout(this.peekTimer);
    this.peeking.set(false);
    this.sketch.set(null);
    if (press.grip && !press.moved) this.tap(press.grip, press.start);
    else if (press.grip?.kind === 'draw') this.addDrawn(press.start, this.toCrop(this.at(event)));
  }

  /** A tap: a shape is selected or unselected, a crossed-out box comes back, the wall clears or marks a point. */
  private tap(held: CropGrip, [x, y]: CropPoint): void {
    const [crop, scene] = [this.draft.crop(), this.draft.draft()];
    if (!crop || !scene) return;
    const selection = this.draft.selection();
    // a handle only drags: tapped, it is what lies under it that was meant (a head above a body's turn handle)
    const handle = held.kind === 'corner' || held.kind === 'turn' || held.kind === 'face';
    const grip = handle ? gripAt(scene, crop, [], [x, y], this.place().scale) : held;
    const id = grip.id;
    if (grip.kind === 'move' && id) {
      this.draft.selection.set(
        selection.includes(id) ? selection.filter((one) => one !== id) : [...selection, id],
      );
    } else if (grip.kind === 'uncross') {
      this.draft.edit((scene) => uncrossed(scene, crop, grip.index));
    } else if (grip.kind === 'draw' && selection.length) {
      this.draft.selection.set([]);
    } else if (grip.kind === 'draw') {
      this.draft.edit((scene) => withPoint(scene, crop, x, y));
    }
  }

  /** A shape drawn on the wall: added, and selected alone so the tools act on it. */
  private addDrawn(start: CropPoint, end: CropPoint): void {
    const box = sketchBox(start, end);
    if (!box) return;
    this.draft.edit((scene) => withDrawn(scene, this.draft.kind(), box));
    const added = this.draft.draft()?.shapes.at(-1);
    if (added) this.draft.selection.set([added.id]);
  }

  /** The wheel zooms about the pointer. */
  protected zoomWheel(event: WheelEvent): void {
    event.preventDefault();
    const [x, y] = this.at(event);
    const from = this.zoom();
    const step = event.deltaY < 0 ? WHEEL_STEP : 1 / WHEEL_STEP;
    const zoom = Math.min(Math.max(from.zoom * step, 1), MAX_ZOOM);
    const grow = zoom / from.zoom;
    this.zoom.set(
      this.clamped({ zoom, left: x - (x - from.left) * grow, top: y - (y - from.top) * grow }),
    );
  }

  /** In a fix, Delete or Backspace removes the selected shapes and Escape unselects them; a field's keys are its own. */
  protected handleKeydown(event: KeyboardEvent): void {
    if (
      this.draft.mode() !== 'fix' ||
      event.defaultPrevented ||
      event.ctrlKey ||
      event.metaKey ||
      event.altKey
    )
      return;
    if (event.target instanceof Element && event.target.closest('input, textarea, select, dialog'))
      return;
    const selection = this.draft.selection();
    if ((event.key === 'Delete' || event.key === 'Backspace') && selection.length) {
      event.preventDefault();
      this.draft.edit((scene) => removed(scene, selection));
      this.draft.selection.set([]);
    } else if (event.key === 'Escape') {
      this.draft.selection.set([]);
    }
  }
}
