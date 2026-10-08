/**
 * The Crops page's canvas (`CropStage`): the crop's picture with its shapes, and every pointer,
 * wheel and key gesture that zooms, pans, peeks and edits them. In: the CropDraft state and the
 * crop's picture from the CropSets contract. Out: edits to the draft's scene and selection; the
 * drawing itself is crop-paint.ts, what a press holds is crop-grip.ts.
 */

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
  defaultSolid,
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
  /** How many times the crop is enlarged (1 to MAX_ZOOM). */
  zoom: number;
  /** Where the crop's left edge is, in CSS pixels from the stage's (0 or less). */
  left: number;
  /** Where the crop's top edge is, in CSS pixels from the stage's (0 or less). */
  top: number;
}

/**
 * One finger's press: what it holds (null outside a fix, or for the mouse's other buttons), whether a drag pans, where
 * it began, and the scene and zoom then.
 */
interface StagePress {
  /** What the press holds; null outside a fix, or for the mouse's pan buttons. */
  grip: CropGrip | null;
  /** Whether a drag pans the view instead of editing. */
  pans: boolean;
  /** Where the press began on the stage. */
  screen: ScreenPoint;
  /** Where the press began on the crop, in crop pixels. */
  start: CropPoint;
  /** The scene when the press began, which a drag edits from; null outside a fix. */
  from: DraftScene | null;
  /** The zoom when the press began, which a pan moves from. */
  zoom: StageZoom;
  /** Whether the finger has moved far enough to drag (DRAG_PX); a press that has not is a tap. */
  moved: boolean;
}

/** The keys held while dragging: Shift keeps sides equal; Alt (or Mirror) moves a side's opposite side too. */
interface DragKeys {
  /** Shift is held: the shape's two sides stay equal. */
  even: boolean;
  /** Alt is held or Mirror is on: a side's opposite side moves too. */
  mirror: boolean;
}

/** Two fingers: how far apart and where their middle was as the second came down, and the zoom. */
interface StagePinch {
  /** How far apart the fingers were, in CSS pixels. */
  distance: number;
  /** Where their middle was on the stage. */
  middle: ScreenPoint;
  /** The zoom then. */
  zoom: StageZoom;
}

/** Which crop's picture to load. */
interface CropImageKey {
  /** The check folder. */
  folder: string;
  /** The crop's id. */
  id: string;
}

/** The tinted mask made for a scene view. */
interface StageMask {
  /** The core's view the mask was made from. */
  view: SceneView;
  /** The mask, tinted, ready to draw over the crop. */
  picture: HTMLCanvasElement;
}

/** The crop at its own size, filling the stage. */
const NO_ZOOM: StageZoom = { zoom: 1, left: 0, top: 0 };
/** The most the crop can be enlarged. */
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
 * crossed-out box back, and on the wall clears the selection or, with none, marks a tiny target.
 * Two fingers (or the wheel) zoom and pan; the
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
  /** The page's state: the crop on show, its scene, the selection and the tools' choices. */
  private readonly draft = inject(CropDraft);
  /** Where the crop's picture comes from. */
  private readonly sets = inject(CropSets);
  /** Ends the canvas's listeners and resize watch with the component. */
  private readonly destroyRef = inject(DestroyRef);
  /** The stage's canvas. */
  private readonly canvas = viewChild.required<ElementRef<HTMLCanvasElement>>('stage');
  /** The zoom and pan; back to none when another crop shows. */
  private readonly zoom = signal<StageZoom>(NO_ZOOM);
  /** The shape a drag on the wall is drawing, before it is added; null when none is. */
  private readonly sketch = signal<Shape | null>(null);
  /** A held finger shows the crop without its marks. */
  private readonly peeking = signal(false);
  /** Space is held: a drag pans. */
  private readonly spaceHeld = signal(false);
  /** Whether a drag pans now, for the cursor. */
  protected readonly pans = computed(() => this.draft.panning() || this.spaceHeld());
  /** Every finger or button down on the stage, by pointer id: where it is now. */
  private readonly pointers = new Map<number, ScreenPoint>();
  /** The one finger's press; null when none is down, or two are. */
  private press: StagePress | null = null;
  /** The two fingers' pinch; null when fewer than two are down. */
  private pinch: StagePinch | null = null;
  /** Starts the peek once a finger has been held PEEK_MS without moving. */
  private peekTimer: ReturnType<typeof setTimeout> | undefined;
  /** The colors and sizes the drawing uses, read from the CSS once. */
  private style: CropStyle | null = null;
  /** The core's last view of the scene, drawn while the next one is worked out. */
  private lastView: SceneView | null = null;
  /** The tinted mask of the last view, made again only when the view changes. */
  private mask: StageMask | null = null;
  /** The crop's picture. */
  private readonly image = resource<ImageBitmap, CropImageKey | undefined>({
    params: () => {
      const [folder, crop] = [this.draft.folder(), this.draft.crop()];
      return folder && crop ? { folder, id: crop.id } : undefined;
    },
    loader: async ({ params }) =>
      createImageBitmap(await this.sets.image(params.folder, params.id)),
  });

  /**
   * Draws again whenever the stage's size or anything it shows changes, listens for pointer moves
   * and the wheel (the wheel not passive: it zooms instead of scrolling), and resets the zoom when
   * another crop shows.
   */
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

  /** Draws the stage at the screen's pixel ratio: the crop, the mask, the shapes and handles. */
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

  /** Where the crop sits on the stage: CSS pixels per crop pixel, and its top left. */
  private place(): StagePlace {
    const side = this.canvas().nativeElement.clientWidth || 1;
    const { zoom, left, top } = this.zoom();
    return { scale: (side / CROP_SIDE) * zoom, left, top };
  }

  /** Where an event happened on the stage. */
  private at(event: MouseEvent): ScreenPoint {
    const bounds = this.canvas().nativeElement.getBoundingClientRect();
    return [event.clientX - bounds.left, event.clientY - bounds.top];
  }

  /** A stage point in crop pixels, kept within the crop. */
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

  /**
   * A finger or button goes down: a second finger starts a pinch; one finger takes hold of what
   * lies under it (in a fix), or pans, and outside a fix starts the peek timer.
   */
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

  /** Drops the press: no peek, no sketch, and a drag's edit undone. */
  private cancelPress(): void {
    clearTimeout(this.peekTimer);
    this.peeking.set(false);
    this.sketch.set(null);
    const press = this.press;
    this.press = null;
    const from = press?.from;
    if (from && press.moved) this.draft.edit(() => from);
  }

  /** A finger moves: the pinch follows, or past DRAG_PX the press drags. */
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
    this.dragTo(press, screen, {
      even: event.shiftKey,
      mirror: event.altKey || this.draft.mirroring(),
    });
  }

  /** Zooms by how far the two fingers spread, about their middle, which pans with them. */
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

  /**
   * A drag to a stage point: it pans, sketches the shape being drawn, or changes the scene from
   * how it was at the press (moved, resized, turned).
   */
  private dragTo(press: StagePress, screen: ScreenPoint, { even, mirror }: DragKeys): void {
    const grip = press.grip;
    if (!grip || press.pans) {
      const [dx, dy] = [screen[0] - press.screen[0], screen[1] - press.screen[1]];
      this.zoom.set(
        this.clamped({ ...press.zoom, left: press.zoom.left + dx, top: press.zoom.top + dy }),
      );
      return;
    }
    const drag = { start: press.start, point: this.toCrop(screen), even, mirror };
    if (grip.kind === 'draw') {
      const box = sketchBox(drag);
      const kind = this.draft.kind();
      const solid = box && this.draft.deep() ? defaultSolid(box) : null;
      this.sketch.set(
        box && {
          id: '',
          kind,
          box,
          points: null,
          angle: 0,
          face: null,
          solid,
          depth: 0,
          role: null,
          model: null,
        },
      );
      return;
    }
    const from = press.from;
    if (from) this.draft.edit(() => dragged(from, grip, this.draft.selection(), drag));
  }

  /**
   * A finger lifts or is cancelled: a press that never moved is a tap, a drag on the wall adds
   * its shape, and a cancelled press is undone.
   */
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
        mirror: false,
      });
  }

  /** A tap: a shape is selected or unselected, a crossed-out box comes back, the wall clears or marks a point. */
  private tap(held: CropGrip, [x, y]: CropPoint): void {
    const [crop, scene] = [this.draft.crop(), this.draft.draft()];
    if (!crop || !scene) return;
    const selection = this.draft.selection();
    // a handle only drags: tapped, it is what lies under it that was meant (a head above a body's turn handle)
    const handle = ['corner', 'side', 'turn', 'face', 'tumble'].includes(held.kind);
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

  /** Space let go: a drag no longer pans. */
  protected handleKeyup(event: KeyboardEvent): void {
    if (event.key !== ' ' || !this.spaceHeld()) return;
    event.preventDefault();
    this.spaceHeld.set(false);
  }

  /** Delete or Backspace removes the selected shapes; Escape unselects them. */
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
