/**
 * Touch on the remote picture. One finger clicks with a tap, drags once it moves, and gives a
 * secondary click when held still; two fingers zoom and pan the picture, and a quick two-finger
 * tap is a secondary click too. Mouse and pen input does not come here.
 *
 * Positions are relative to the surface's top-left corner; times are milliseconds.
 */

/** Movement below this many pixels still counts as a tap or a hold. */
export const SLOP = 10;
/** Holding one finger this long is a secondary click. */
export const HOLD = 550;
/** Two fingers lifted within this long of touching, without zooming, are a secondary click. */
export const TWO_TAP = 300;
export const MAX_SCALE = 4;

export type Point = { x: number; y: number };
/** The picture's zoom: `scale` around the surface centre, then moved by `x`, `y`. */
export type View = { scale: number; x: number; y: number };
export const IDENTITY: View = { scale: 1, x: 0, y: 0 };

/** A pointer state for the remote computer, at a surface position. */
export type Press = Point & { buttons: number };
export type Output = { press: Press } | { view: View };

type Touch = { start: Point; at: number; now: Point };

type Mode =
  | { kind: "idle" }
  | { kind: "pending" }
  | { kind: "drag" }
  | {
      kind: "pinch";
      distance: number;
      mid: Point;
      view: View;
      at: number;
      zoomed: boolean;
    }
  /** Done with this gesture: wait until every finger is up. */
  | { kind: "spent" };

const distance = (a: Point, b: Point) => Math.hypot(a.x - b.x, a.y - b.y);
const middle = (a: Point, b: Point) => ({
  x: (a.x + b.x) / 2,
  y: (a.y + b.y) / 2,
});

/** Keeps the zoomed picture covering the surface. */
export function clampView(
  view: View,
  size: { width: number; height: number },
): View {
  const scale = Math.min(MAX_SCALE, Math.max(1, view.scale));
  const limit = (extent: number) => ((scale - 1) * extent) / 2;
  const clamp = (value: number, bound: number) =>
    Math.max(-bound, Math.min(bound, value));
  return {
    scale,
    x: clamp(view.x, limit(size.width)),
    y: clamp(view.y, limit(size.height)),
  };
}

/** Zooms by `factor` around the surface centre, as the zoom buttons do. */
export function zoomBy(
  view: View,
  factor: number,
  size: { width: number; height: number },
) {
  const scale = Math.min(MAX_SCALE, Math.max(1, view.scale * factor));
  const ratio = scale / view.scale;
  return clampView({ scale, x: view.x * ratio, y: view.y * ratio }, size);
}

const click = (at: Point, buttons: number): Output[] => [
  { press: { ...at, buttons: 0 } },
  { press: { ...at, buttons } },
  { press: { ...at, buttons: 0 } },
];

export class Gestures {
  private touches = new Map<number, Touch>();
  private mode: Mode = { kind: "idle" };
  view: View = IDENTITY;
  private size: () => { width: number; height: number };

  constructor(size: () => { width: number; height: number }) {
    this.size = size;
  }

  setView(view: View) {
    this.view = clampView(view, this.size());
  }

  down(id: number, point: Point, at: number): Output[] {
    this.touches.set(id, { start: point, at, now: point });
    if (this.touches.size === 1) {
      this.mode = { kind: "pending" };
      return [];
    }
    if (this.touches.size === 2 && this.mode.kind === "pending") {
      const [a, b] = [...this.touches.values()];
      this.mode = {
        kind: "pinch",
        distance: Math.max(1, distance(a.now, b.now)),
        mid: middle(a.now, b.now),
        view: this.view,
        at,
        zoomed: false,
      };
      return [];
    }
    // A finger added to a drag or a third finger ends the gesture.
    const out = this.release();
    this.mode = { kind: "spent" };
    return out;
  }

  move(id: number, point: Point): Output[] {
    const touch = this.touches.get(id);
    if (!touch) return [];
    touch.now = point;
    const mode = this.mode;
    if (mode.kind === "pending" && distance(touch.start, point) > SLOP) {
      this.mode = { kind: "drag" };
      return [
        { press: { ...touch.start, buttons: 0 } },
        { press: { ...touch.start, buttons: 1 } },
        { press: { ...point, buttons: 1 } },
      ];
    }
    if (mode.kind === "drag") return [{ press: { ...point, buttons: 1 } }];
    if (mode.kind === "pinch" && this.touches.size === 2) {
      const [a, b] = [...this.touches.values()];
      const mid = middle(a.now, b.now);
      const scale = mode.view.scale * (distance(a.now, b.now) / mode.distance);
      const size = this.size();
      const center = { x: size.width / 2, y: size.height / 2 };
      // The picture point under the starting midpoint stays under the fingers.
      const content = {
        x: (mode.mid.x - center.x - mode.view.x) / mode.view.scale,
        y: (mode.mid.y - center.y - mode.view.y) / mode.view.scale,
      };
      const next = clampView(
        {
          scale,
          x: mid.x - center.x - content.x * scale,
          y: mid.y - center.y - content.y * scale,
        },
        size,
      );
      if (
        Math.abs(next.scale - mode.view.scale) > 0.02 ||
        distance(mid, mode.mid) > SLOP
      )
        mode.zoomed = true;
      this.view = next;
      return [{ view: next }];
    }
    return [];
  }

  up(id: number, point: Point, at: number): Output[] {
    const touch = this.touches.get(id);
    if (!touch) return [];
    touch.now = point;
    const mode = this.mode;
    let out: Output[] = [];
    if (mode.kind === "pending") {
      out = click(touch.start, 1);
      this.mode = { kind: "spent" };
    } else if (mode.kind === "drag") {
      out = [{ press: { ...point, buttons: 0 } }];
      this.mode = { kind: "spent" };
    } else if (mode.kind === "pinch") {
      if (!mode.zoomed && at - mode.at <= TWO_TAP) {
        const [a, b] = [...this.touches.values()];
        out = click(middle(a.start, b.start), 2);
      }
      this.mode = { kind: "spent" };
    }
    this.touches.delete(id);
    if (this.touches.size === 0) this.mode = { kind: "idle" };
    return out;
  }

  /** Called on a timer: a still finger held long enough is a secondary click. */
  tick(at: number): Output[] {
    if (this.mode.kind !== "pending" || this.touches.size !== 1) return [];
    const [touch] = [...this.touches.values()];
    if (at - touch.at < HOLD) return [];
    this.mode = { kind: "spent" };
    return click(touch.start, 2);
  }

  /** The browser took the touch away: whatever was pressed is let go. */
  cancel(): Output[] {
    const out = this.release();
    this.touches.clear();
    this.mode = { kind: "idle" };
    return out;
  }

  private release(): Output[] {
    if (this.mode.kind !== "drag") return [];
    const [touch] = [...this.touches.values()];
    return touch ? [{ press: { ...touch.now, buttons: 0 } }] : [];
  }
}
