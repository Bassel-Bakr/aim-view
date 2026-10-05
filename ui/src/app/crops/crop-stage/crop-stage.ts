import {
  afterNextRender,
  afterRenderEffect,
  Component,
  computed,
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
import { listenQuietly } from '../../services/listen-quietly';
import { CropPoint } from '../../shapes/shape-geometry';
import { CROP_SIDE, CropDraft } from '../crop-draft';
import {
  defaultFace,
  DraftScene,
  removed,
  resizedAll,
  uncrossed,
  withDrawn,
  withPoint,
} from '../crop-scene';
import { CropDrag, CropGrip, dragged, gripAt, handleReach, shapesAt, sketchBox } from './crop-grip';
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

/**
 * One finger's press: what it holds (null outside a fix, or for the mouse's other buttons), whether a drag pans, where
 * it began, and the scene and zoom then.
 */
interface StagePress {
  grip: CropGrip | null;
  pans: boolean;
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
/** One wheel step over the selected shapes resizes them by this much. */
const RESIZE_STEP = 1.1;
/** The mouse's middle and right buttons pan. */
const PAN_BUTTONS = [1, 2];
/** A finger moving this far (CSS pixels) drags; less is a tap. */
const DRAG_PX = 6;
/** A finger held this long (ms) without moving shows the crop without its marks. */
const PEEK_MS = 180;

/**
 * The crop on show and its shapes, on a canvas. Outside a fix, a held finger peeks under the marks and a moving one
 * pans a zoomed crop. In a fix: a drag on the wall draws a shape of the chosen kind, a drag on a shape moves it (with
 * the selection), and a shape selected alone shows handles: its corners resize it, its handle above turns it and its
 * square handle pulls out its third face (a 3D box's or pill's far end). A tap selects or unselects a shape, brings a
 * crossed-out box back, and on the wall clears the selection or, with none, marks a tiny target. Two fingers (or the wheel) zoom and pan; the
 * tools' Pan, Space, or the mouse's middle or right button make a drag pan; the wheel over the selected shapes resizes
 * them. Shift while drawing or resizing keeps a shape's two sides equal (a perfect circle or square). Delete removes the
 * selected shapes, Ctrl+D duplicates them.
 */
@Component({
  selector: 'app-crop-stage',
  templateUrl: './crop-stage.html',
  styleUrl: './crop-stage.scss',
  host: {
    '(document:keydown)': 'handleKeydown($event)',
    '(document:keyup)': 'handleKeyup($event)',
  },
})
export class CropStage {
  private readonly draft = inject(CropDraft);
  private readonly sets = inject(CropSets);
  private readonly destroyRef = inject(DestroyRef);
  private readonly canvas = viewChild.required<ElementRef<HTMLCanvasElement>>('stage');
  private readonly zoom = signal<StageZoom>(NO_ZOOM);
  private readonly sketch = signal<Shape | null>(null);
  private readonly peeking = signal(false);
  /** Space is held: a drag pans. */
  private readonly spaceHeld = signal(false);
  /** Whether a drag pans now, for the cursor. */
  protected readonly pans = computed(() => this.draft.panning() || this.spaceHeld());
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
      const canvas = this.canvas().nativeElement;
      const resize = new ResizeObserver(() => this.draw());
      resize.observe(canvas);
      this.destroyRef.onDestroy(() => resize.disconnect());
      listenQuietly(canvas, 'pointermove', (event) => this.movePointer(event), this.destroyRef);
      listenQuietly(canvas, 'wheel', (event) => this.turnWheel(event), this.destroyRef, {
        passive: false,
      });
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
    const panButton = event.pointerType === 'mouse' && PAN_BUTTONS.includes(event.button);
    if (event.pointerType === 'mouse' && event.button !== 0 && !panButton) return;
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
      scene && crop && !panButton
        ? gripAt(
            scene,
            crop,
            this.draft.selection(),
            start,
            this.place().scale,
            handleReach(event.pointerType),
          )
        : null;
    const pans = !grip || this.pans();
    this.press = { grip, pans, screen, start, from: scene, zoom: this.zoom(), moved: false };
    if (!scene && !panButton)
      this.peekTimer = setTimeout(() => this.peeking.set(this.press?.moved === false), PEEK_MS);
  }

  /** The middle button would start the browser's own scrolling: here it pans. */
  protected holdMiddle(event: MouseEvent): void {
    if (event.button === 1) event.preventDefault();
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

  private movePointer(event: PointerEvent): void {
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
    this.dragTo(press, screen, event.shiftKey);
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

  private dragTo(press: StagePress, screen: ScreenPoint, even: boolean): void {
    const grip = press.grip;
    if (!grip || press.pans) {
      const [dx, dy] = [screen[0] - press.screen[0], screen[1] - press.screen[1]];
      this.zoom.set(
        this.clamped({ ...press.zoom, left: press.zoom.left + dx, top: press.zoom.top + dy }),
      );
      return;
    }
    const drag = { start: press.start, point: this.toCrop(screen), even };
    if (grip.kind === 'draw') {
      const box = sketchBox(drag);
      const kind = this.draft.kind();
      const face = box && this.draft.deep() ? defaultFace(box) : null;
      this.sketch.set(
        box && { id: '', kind, box, angle: 0, face, depth: 0, role: null, model: null },
      );
      return;
    }
    const from = press.from;
    if (from) this.draft.edit(() => dragged(from, grip, this.draft.selection(), drag));
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
    else if (!press.pans && press.grip?.kind === 'draw')
      this.addDrawn({
        start: press.start,
        point: this.toCrop(this.at(event)),
        even: event.shiftKey,
      });
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
  private addDrawn(drag: CropDrag): void {
    const box = sketchBox(drag);
    if (!box) return;
    this.draft.edit((scene) => withDrawn(scene, this.draft.kind(), box, this.draft.deep()));
    const added = this.draft.draft()?.shapes.at(-1);
    if (added) this.draft.selection.set([added.id]);
  }

  /** The wheel over the selected shapes resizes them; elsewhere, or with Ctrl (a touchpad's pinch), it zooms. */
  private turnWheel(event: WheelEvent): void {
    event.preventDefault();
    const larger = event.deltaY < 0;
    if (!event.ctrlKey && this.resizeAt(this.at(event), larger ? RESIZE_STEP : 1 / RESIZE_STEP))
      return;
    this.zoomAt(this.at(event), larger ? WHEEL_STEP : 1 / WHEEL_STEP);
  }

  /** Resizes the selected shapes when the point is on one of them; false when it is not (or outside a fix). */
  private resizeAt(screen: ScreenPoint, factor: number): boolean {
    const [scene, ids] = [
      this.draft.mode() === 'fix' ? this.draft.draft() : null,
      this.draft.selection(),
    ];
    const under = scene ? shapesAt(scene, this.toCrop(screen), this.place().scale) : [];
    if (!under.some((shape) => ids.includes(shape.id))) return false;
    this.draft.edit((now) => resizedAll(now, ids, factor));
    return true;
  }

  /** Zooms by a step about a point of the stage. */
  private zoomAt([x, y]: ScreenPoint, step: number): void {
    const from = this.zoom();
    const zoom = Math.min(Math.max(from.zoom * step, 1), MAX_ZOOM);
    const grow = zoom / from.zoom;
    this.zoom.set(
      this.clamped({ zoom, left: x - (x - from.left) * grow, top: y - (y - from.top) * grow }),
    );
  }

  /**
   * In a fix: Delete or Backspace removes the selected shapes, Escape unselects them, Ctrl+D duplicates them, and a drag
   * with Space held pans. A field's keys are its own.
   */
  protected handleKeydown(event: KeyboardEvent): void {
    if (this.draft.mode() !== 'fix' || event.defaultPrevented || event.altKey) return;
    if (event.target instanceof Element && event.target.closest('input, textarea, select, dialog'))
      return;
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'd') {
      event.preventDefault();
      this.draft.duplicate();
    } else if (event.key === ' ') {
      // neither the focused button's click nor the page's scroll
      event.preventDefault();
      this.spaceHeld.set(true);
    } else if (!event.ctrlKey && !event.metaKey) {
      this.editByKey(event);
    }
  }

  protected handleKeyup(event: KeyboardEvent): void {
    if (event.key !== ' ' || !this.spaceHeld()) return;
    event.preventDefault();
    this.spaceHeld.set(false);
  }

  private editByKey(event: KeyboardEvent): void {
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
