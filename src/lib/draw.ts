// --- Drawing overlay: freehand strokes that interfere with floating letters ---

interface Point {
  x: number;
  y: number;
}

interface BBox {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

interface Stroke {
  points: Point[];
  closed: boolean;
  bbox: BBox;
  createdAt: number;
  fadeStart: number;
}

const DRAG_THRESHOLD = 5;
const MIN_POINT_DIST = 8;
const CLOSE_DIST = 30;
const MIN_LOOP_AREA = 2000;
const OPEN_LIFETIME = 8000;
const CLOSED_LIFETIME = 15000;
const FADE_DURATION = 3000;
const REPEL_RADIUS = 40;

const strokes: Stroke[] = [];
let currentPoints: Point[] | null = null;
let drawing = false;
let mouseDownPos: Point | null = null;
let canvas: HTMLCanvasElement | null = null;
let ctx: CanvasRenderingContext2D | null = null;
let raf = 0;
let touchPos: { x: number; y: number } | null = null;

// --- Geometry helpers ---

function closestPointOnSegment(
  px: number,
  py: number,
  ax: number,
  ay: number,
  bx: number,
  by: number,
): { x: number; y: number; dist: number } {
  const abx = bx - ax;
  const aby = by - ay;
  const len2 = abx * abx + aby * aby;
  if (len2 < 0.001) {
    const dx = px - ax;
    const dy = py - ay;
    return { x: ax, y: ay, dist: Math.sqrt(dx * dx + dy * dy) };
  }
  let t = ((px - ax) * abx + (py - ay) * aby) / len2;
  t = Math.max(0, Math.min(1, t));
  const cx = ax + t * abx;
  const cy = ay + t * aby;
  const dx = px - cx;
  const dy = py - cy;
  return { x: cx, y: cy, dist: Math.sqrt(dx * dx + dy * dy) };
}

function pointInPolygon(x: number, y: number, poly: Point[]): boolean {
  let inside = false;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    const xi = poly[i].x, yi = poly[i].y;
    const xj = poly[j].x, yj = poly[j].y;
    if ((yi > y) !== (yj > y) && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) {
      inside = !inside;
    }
  }
  return inside;
}

function polygonArea(poly: Point[]): number {
  let area = 0;
  for (let i = 0, j = poly.length - 1; i < poly.length; j = i++) {
    area += poly[j].x * poly[i].y - poly[i].x * poly[j].y;
  }
  return Math.abs(area) / 2;
}

function computeBBox(points: Point[], pad: number): BBox {
  let minX = Infinity, minY = Infinity, maxX = -Infinity, maxY = -Infinity;
  for (const p of points) {
    if (p.x < minX) minX = p.x;
    if (p.y < minY) minY = p.y;
    if (p.x > maxX) maxX = p.x;
    if (p.y > maxY) maxY = p.y;
  }
  return { minX: minX - pad, minY: minY - pad, maxX: maxX + pad, maxY: maxY + pad };
}

function randomPointInPolygon(poly: Point[], bbox: BBox): Point {
  for (let i = 0; i < 100; i++) {
    const x = bbox.minX + Math.random() * (bbox.maxX - bbox.minX);
    const y = bbox.minY + Math.random() * (bbox.maxY - bbox.minY);
    if (pointInPolygon(x, y, poly)) return { x, y };
  }
  // fallback: centroid
  let cx = 0, cy = 0;
  for (const p of poly) { cx += p.x; cy += p.y; }
  return { x: cx / poly.length, y: cy / poly.length };
}

// --- Stroke opacity ---

function strokeOpacity(s: Stroke, now: number): number {
  const lifetime = s.closed ? CLOSED_LIFETIME : OPEN_LIFETIME;
  const age = now - s.createdAt;
  if (age < s.fadeStart) return 1;
  const fadeAge = age - s.fadeStart;
  if (fadeAge >= FADE_DURATION) return 0;
  return 1 - fadeAge / FADE_DURATION;
}

// --- Mouse handlers ---

function onMouseDown(e: MouseEvent) {
  // Don't intercept clicks on links
  if ((e.target as HTMLElement).closest("a")) return;
  mouseDownPos = { x: e.clientX, y: e.clientY };
}

function onMouseMove(e: MouseEvent) {
  if (!mouseDownPos) return;

  const dx = e.clientX - mouseDownPos.x;
  const dy = e.clientY - mouseDownPos.y;

  if (!drawing && Math.sqrt(dx * dx + dy * dy) >= DRAG_THRESHOLD) {
    drawing = true;
    currentPoints = [{ x: mouseDownPos.x, y: mouseDownPos.y }];
  }

  if (drawing && currentPoints) {
    const last = currentPoints[currentPoints.length - 1];
    const pdx = e.clientX - last.x;
    const pdy = e.clientY - last.y;
    if (pdx * pdx + pdy * pdy >= MIN_POINT_DIST * MIN_POINT_DIST) {
      currentPoints.push({ x: e.clientX, y: e.clientY });
    }
  }
}

function onMouseUp() {
  if (drawing && currentPoints && currentPoints.length > 2) {
    const first = currentPoints[0];
    const last = currentPoints[currentPoints.length - 1];
    const closeDist = Math.sqrt(
      (first.x - last.x) ** 2 + (first.y - last.y) ** 2,
    );
    const isClosed =
      closeDist <= CLOSE_DIST && polygonArea(currentPoints) >= MIN_LOOP_AREA;

    const now = performance.now();
    const lifetime = isClosed ? CLOSED_LIFETIME : OPEN_LIFETIME;

    strokes.push({
      points: currentPoints,
      closed: isClosed,
      bbox: computeBBox(currentPoints, REPEL_RADIUS),
      createdAt: now,
      fadeStart: lifetime - FADE_DURATION,
    });
  }

  drawing = false;
  currentPoints = null;
  mouseDownPos = null;
}

// --- Touch handlers ---

function onTouchStart(e: TouchEvent) {
  if ((e.target as HTMLElement).closest("a")) return;
  const t = e.touches[0];
  mouseDownPos = { x: t.clientX, y: t.clientY };
  touchPos = { x: t.clientX, y: t.clientY };
}

function onTouchMove(e: TouchEvent) {
  const t = e.touches[0];
  touchPos = { x: t.clientX, y: t.clientY };

  if (!mouseDownPos) return;

  const dx = t.clientX - mouseDownPos.x;
  const dy = t.clientY - mouseDownPos.y;

  if (!drawing && Math.sqrt(dx * dx + dy * dy) >= DRAG_THRESHOLD) {
    drawing = true;
    currentPoints = [{ x: mouseDownPos.x, y: mouseDownPos.y }];
  }

  if (drawing && currentPoints) {
    e.preventDefault(); // suppress scrolling while drawing
    const last = currentPoints[currentPoints.length - 1];
    const pdx = t.clientX - last.x;
    const pdy = t.clientY - last.y;
    if (pdx * pdx + pdy * pdy >= MIN_POINT_DIST * MIN_POINT_DIST) {
      currentPoints.push({ x: t.clientX, y: t.clientY });
    }
  }
}

function onTouchEnd() {
  if (drawing && currentPoints && currentPoints.length > 2) {
    const first = currentPoints[0];
    const last = currentPoints[currentPoints.length - 1];
    const closeDist = Math.sqrt(
      (first.x - last.x) ** 2 + (first.y - last.y) ** 2,
    );
    const isClosed =
      closeDist <= CLOSE_DIST && polygonArea(currentPoints) >= MIN_LOOP_AREA;

    const now = performance.now();
    const lifetime = isClosed ? CLOSED_LIFETIME : OPEN_LIFETIME;

    strokes.push({
      points: currentPoints,
      closed: isClosed,
      bbox: computeBBox(currentPoints, REPEL_RADIUS),
      createdAt: now,
      fadeStart: lifetime - FADE_DURATION,
    });
  }

  drawing = false;
  currentPoints = null;
  mouseDownPos = null;
  touchPos = null;
}

// --- Render ---

function render(now: number) {
  if (!ctx || !canvas) return;
  ctx.clearRect(0, 0, canvas.width, canvas.height);

  // Remove expired strokes
  for (let i = strokes.length - 1; i >= 0; i--) {
    if (strokeOpacity(strokes[i], now) <= 0) {
      strokes.splice(i, 1);
    }
  }

  // Draw completed strokes
  for (const s of strokes) {
    const alpha = strokeOpacity(s, now);
    if (alpha <= 0) continue;
    drawStroke(s.points, s.closed, alpha);
  }

  // Draw in-progress stroke
  if (drawing && currentPoints && currentPoints.length > 1) {
    drawStroke(currentPoints, false, 1);
  }

  raf = requestAnimationFrame(render);
}

function drawStroke(points: Point[], closed: boolean, alpha: number) {
  if (!ctx || points.length < 2) return;

  // Smooth curve through midpoints using quadratic beziers
  ctx.beginPath();
  ctx.moveTo(points[0].x, points[0].y);

  if (points.length === 2) {
    ctx.lineTo(points[1].x, points[1].y);
  } else {
    // First segment: curve to midpoint of first two points
    let midX = (points[0].x + points[1].x) / 2;
    let midY = (points[0].y + points[1].y) / 2;
    ctx.quadraticCurveTo(points[0].x, points[0].y, midX, midY);

    // Middle segments: curve through midpoints using points as control
    for (let i = 1; i < points.length - 1; i++) {
      midX = (points[i].x + points[i + 1].x) / 2;
      midY = (points[i].y + points[i + 1].y) / 2;
      ctx.quadraticCurveTo(points[i].x, points[i].y, midX, midY);
    }

    // Last segment: curve to final point
    const last = points[points.length - 1];
    ctx.quadraticCurveTo(last.x, last.y, last.x, last.y);
  }

  if (closed) ctx.closePath();

  ctx.strokeStyle = `rgba(68, 68, 68, ${alpha * 0.5})`;
  ctx.lineWidth = 5;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  ctx.stroke();

  if (closed) {
    ctx.fillStyle = `rgba(68, 68, 68, ${0.03 * alpha})`;
    ctx.fill();
  }
}

// --- Resize ---

function resize() {
  if (!canvas) return;
  canvas.width = window.innerWidth;
  canvas.height = window.innerHeight;
}

// --- Public API ---

export function getLineRepulsion(
  cx: number,
  cy: number,
): { fx: number; fy: number; proximity: number } {
  let fx = 0;
  let fy = 0;
  let maxProximity = 0;
  const now = performance.now();

  for (const s of strokes) {
    const alpha = strokeOpacity(s, now);
    if (alpha <= 0) continue;

    // BBox early-out
    if (
      cx < s.bbox.minX || cx > s.bbox.maxX ||
      cy < s.bbox.minY || cy > s.bbox.maxY
    ) continue;

    const pts = s.points;
    for (let i = 0; i < pts.length - 1; i++) {
      const cp = closestPointOnSegment(
        cx, cy,
        pts[i].x, pts[i].y,
        pts[i + 1].x, pts[i + 1].y,
      );
      if (cp.dist < REPEL_RADIUS && cp.dist > 0.5) {
        const strength = (1 - cp.dist / REPEL_RADIUS) * alpha;
        const dx = cx - cp.x;
        const dy = cy - cp.y;
        fx += (dx / cp.dist) * strength * 3;
        fy += (dy / cp.dist) * strength * 3;
        if (strength > maxProximity) maxProximity = strength;
      }
    }
  }

  // Also repel from in-progress stroke
  if (drawing && currentPoints && currentPoints.length > 1) {
    for (let i = 0; i < currentPoints.length - 1; i++) {
      const cp = closestPointOnSegment(
        cx, cy,
        currentPoints[i].x, currentPoints[i].y,
        currentPoints[i + 1].x, currentPoints[i + 1].y,
      );
      if (cp.dist < REPEL_RADIUS && cp.dist > 0.5) {
        const strength = 1 - cp.dist / REPEL_RADIUS;
        const dx = cx - cp.x;
        const dy = cy - cp.y;
        fx += (dx / cp.dist) * strength * 3;
        fy += (dy / cp.dist) * strength * 3;
        if (strength > maxProximity) maxProximity = strength;
      }
    }
  }

  return { fx, fy, proximity: maxProximity };
}

export function getContainingLoop(
  x: number,
  y: number,
): Stroke | null {
  const now = performance.now();
  for (const s of strokes) {
    if (!s.closed) continue;
    if (strokeOpacity(s, now) <= 0) continue;
    if (
      x < s.bbox.minX + REPEL_RADIUS || x > s.bbox.maxX - REPEL_RADIUS ||
      y < s.bbox.minY + REPEL_RADIUS || y > s.bbox.maxY - REPEL_RADIUS
    ) continue;
    if (pointInPolygon(x, y, s.points)) return s;
  }
  return null;
}

export function getRandomPointInLoop(loop: Stroke): Point {
  return randomPointInPolygon(loop.points, loop.bbox);
}

export function isCurrentlyDrawing(): boolean {
  return drawing;
}

export function getTouchPos(): { x: number; y: number } | null {
  return touchPos;
}

// --- Init / cleanup ---

export function initDraw(): () => void {
  canvas = document.createElement("canvas");
  canvas.style.cssText =
    "position:fixed;top:0;left:0;width:100%;height:100%;z-index:1;pointer-events:none;";
  document.body.appendChild(canvas);
  ctx = canvas.getContext("2d");

  resize();
  window.addEventListener("resize", resize);
  window.addEventListener("mousedown", onMouseDown);
  window.addEventListener("mousemove", onMouseMove);
  window.addEventListener("mouseup", onMouseUp);
  window.addEventListener("touchstart", onTouchStart, { passive: true });
  window.addEventListener("touchmove", onTouchMove, { passive: false });
  window.addEventListener("touchend", onTouchEnd);

  raf = requestAnimationFrame(render);

  return () => {
    cancelAnimationFrame(raf);
    window.removeEventListener("resize", resize);
    window.removeEventListener("mousedown", onMouseDown);
    window.removeEventListener("mousemove", onMouseMove);
    window.removeEventListener("mouseup", onMouseUp);
    window.removeEventListener("touchstart", onTouchStart);
    window.removeEventListener("touchmove", onTouchMove);
    window.removeEventListener("touchend", onTouchEnd);
    canvas?.remove();
    canvas = null;
    ctx = null;
    strokes.length = 0;
  };
}
