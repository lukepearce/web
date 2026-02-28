import { isCurrentlyDrawing, getTouchPos } from "./draw";

const GLYPHS = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%&*+=?/";
const GLYPH_COUNT = 12;
const BASE_RADIUS = 32;

function randomGlyph(): string {
  return GLYPHS[Math.floor(Math.random() * GLYPHS.length)];
}

function smoothstep(edge0: number, edge1: number, x: number): number {
  const t = Math.max(0, Math.min(1, (x - edge0) / (edge1 - edge0)));
  return t * t * (3 - 2 * t);
}

export function initCursor(): () => void {
  // Hide default cursor globally
  const cursorStyle = document.createElement("style");
  cursorStyle.textContent =
    "@media (pointer: fine) { *, *::before, *::after { cursor: none !important; } }";
  document.head.appendChild(cursorStyle);

  const mousePos = { x: -9999, y: -9999 };
  let hasMoved = false;
  let hasTouch = false;

  function onMouseMove(e: MouseEvent) {
    mousePos.x = e.clientX;
    mousePos.y = e.clientY;
    if (!hasMoved) hasMoved = true;
  }
  window.addEventListener("mousemove", onMouseMove);

  // Container
  const container = document.createElement("div");
  container.style.cssText =
    "position:fixed;top:0;left:0;pointer-events:none;z-index:9999;opacity:0;";
  document.body.appendChild(container);

  // Glyph spans — orbit in a ring
  const glyphEls: HTMLSpanElement[] = [];
  for (let i = 0; i < GLYPH_COUNT; i++) {
    const span = document.createElement("span");
    span.style.cssText =
      "position:absolute;font-size:9px;line-height:1;font-family:var(--font-mono);color:#444;";
    span.textContent = randomGlyph();
    container.appendChild(span);
    glyphEls.push(span);
  }

  // Center dot
  const dot = document.createElement("div");
  dot.style.cssText =
    "position:absolute;width:3px;height:3px;background:#666;border-radius:50%;transform:translate(-1.5px,-1.5px);";
  container.appendChild(dot);

  let cx = -9999;
  let cy = -9999;
  let rotation = 0;
  let lastGlitch = 0;
  let prevTime = 0;
  let prevGlyphColor = "";
  let currentRadius = BASE_RADIUS;
  let currentDrawBoost = 0;
  let currentOpacity = 0;

  function tick(time: number) {
    const dt = prevTime ? time - prevTime : 16;
    prevTime = time;

    // Touch position override
    const tp = getTouchPos();
    if (tp) {
      mousePos.x = tp.x;
      mousePos.y = tp.y;
      hasTouch = true;
      if (!hasMoved) hasMoved = true;
    }

    // Opacity: on touch devices, show while touching and fade on release
    // On mouse devices, show once mouse has moved
    const targetOpacity = hasTouch ? (tp ? 1 : 0) : (hasMoved ? 1 : 0);
    currentOpacity += (targetOpacity - currentOpacity) * 0.12;
    container.style.opacity = `${currentOpacity}`;

    // Smooth follow
    cx += (mousePos.x - cx) * 0.15;
    cy += (mousePos.y - cy) * 0.15;
    container.style.transform = `translate(${cx}px, ${cy}px)`;

    // Distance to nearest link
    const links = document.querySelectorAll<HTMLElement>("nav a");
    let minDist = Infinity;
    links.forEach((link) => {
      const rect = link.getBoundingClientRect();
      const lx = rect.left + rect.width / 2;
      const ly = rect.top + rect.height / 2;
      const d = Math.sqrt(
        (mousePos.x - lx) ** 2 + (mousePos.y - ly) ** 2,
      );
      if (d < minDist) minDist = d;
    });

    // proximity: 0 = on top of link, 1 = far away
    const proximity = smoothstep(25, 120, minDist);

    // Rotation — slower near links
    rotation += (0.0003 + proximity * 0.0008) * dt;

    // Expand ring while drawing — lerp for smooth transition
    const isDrawing = isCurrentlyDrawing();
    const targetRadius = isDrawing ? BASE_RADIUS * 1.4 : BASE_RADIUS;
    currentRadius += (targetRadius - currentRadius) * 0.08;
    const radius = currentRadius;

    // Position glyphs on ring, fade out near links
    const glyphOpacity = proximity;
    glyphEls.forEach((el, i) => {
      const angle = (i / GLYPH_COUNT) * Math.PI * 2 + rotation;
      const gx = Math.cos(angle) * radius - 3;
      const gy = Math.sin(angle) * radius - 5;
      el.style.transform = `translate(${gx}px, ${gy}px)`;
      el.style.opacity = `${glyphOpacity}`;
    });

    // Cycle glyphs — rapid when far, slow when near
    const glitchInterval = 40 + (1 - proximity) * 200;
    if (time - lastGlitch > glitchInterval) {
      lastGlitch = time;
      const swapCount = proximity > 0.5 ? 3 : 1;
      for (let j = 0; j < swapCount; j++) {
        const idx = Math.floor(Math.random() * GLYPH_COUNT);
        glyphEls[idx].textContent = randomGlyph();
      }
    }

    // Glyph color
    const brightness = Math.round(0x33 + (1 - proximity) * 0x55);
    const hex = brightness.toString(16).padStart(2, "0");
    const glyphColor = `#${hex}${hex}${hex}`;
    if (glyphColor !== prevGlyphColor) {
      prevGlyphColor = glyphColor;
      glyphEls.forEach((el) => (el.style.color = glyphColor));
    }

    // Center dot — pulse when near links, brighten when drawing
    const pulse = Math.sin(time * 0.006) * 0.5 + 0.5; // 0–1 oscillation
    const dotScale = 1 + (1 - proximity) * pulse * 1.5; // up to 2.5x when near
    const targetDrawBoost = isDrawing ? 0x44 : 0;
    currentDrawBoost += (targetDrawBoost - currentDrawBoost) * 0.08;
    const dotBright = Math.round(0x55 + (1 - proximity) * pulse * 0xaa + currentDrawBoost);
    const dotHex = Math.min(0xff, dotBright).toString(16).padStart(2, "0");
    dot.style.transform = `translate(-1.5px,-1.5px) scale(${dotScale})`;
    dot.style.background = `#${dotHex}${dotHex}${dotHex}`;

    raf = requestAnimationFrame(tick);
  }

  let raf = requestAnimationFrame(tick);

  return () => {
    cancelAnimationFrame(raf);
    window.removeEventListener("mousemove", onMouseMove);
    container.remove();
    cursorStyle.remove();
  };
}
