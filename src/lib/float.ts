const GLYPHS = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%&*+=?/";

// Shared mouse position — single listener, all elements read from it
const mouse = { x: -9999, y: -9999 };
let listening = false;

function ensureMouseListener() {
  if (listening) return;
  listening = true;
  window.addEventListener("mousemove", (e) => {
    mouse.x = e.clientX;
    mouse.y = e.clientY;
  });
}

function smoothstep(edge0: number, edge1: number, x: number): number {
  const t = Math.max(0, Math.min(1, (x - edge0) / (edge1 - edge0)));
  return t * t * (3 - 2 * t);
}

function rand(min: number, max: number): number {
  return min + Math.random() * (max - min);
}

function randSign(): number {
  return Math.random() < 0.5 ? -1 : 1;
}

function randomGlyph(): string {
  return GLYPHS[Math.floor(Math.random() * GLYPHS.length)];
}

const INNER_RADIUS = 25;
const OUTER_RADIUS = 100;

// --- Shared link registry for repulsion ---

interface WanderEntry {
  node: HTMLElement;
  x: number;
  y: number;
  targetX: number;
  targetY: number;
}

const wanderEntries: WanderEntry[] = [];
const MIN_LINK_DIST = 18; // minimum % distance between link centers
const WANDER_LERP = 0.00003; // very slow approach per ms

// --- Sine channel system for layered, non-repeating motion ---

interface Channel {
  freq: number;
  phase: number;
  amp: number;
}

function makeChannels(
  count: number,
  freqMin: number,
  freqMax: number,
  ampMin: number,
  ampMax: number,
): Channel[] {
  return Array.from({ length: count }, () => ({
    freq: rand(freqMin, freqMax),
    phase: rand(0, Math.PI * 2),
    amp: rand(ampMin, ampMax) * randSign(),
  }));
}

function evalChannels(channels: Channel[], time: number): number {
  return channels.reduce(
    (sum, ch) => sum + Math.sin(time * ch.freq + ch.phase) * ch.amp,
    0,
  );
}

// --- Per-character state ---

interface CharState {
  span: HTMLSpanElement;
  originalChar: string;
  xChannels: Channel[];
  yChannels: Channel[];
  zChannels: Channel[];
  rotXChannels: Channel[];
  rotYChannels: Channel[];
  rotZChannels: Channel[];
  scaleChannel: Channel;
  skewChannel: Channel;
  calmPhase: number;
  lastGlitchCheck: number;
  isGlitched: boolean;
  glitchChar: string;
  glitchInterval: number;
  glitchProb: number;
}

// --- DOM splitting ---

function splitChars(node: HTMLElement): HTMLSpanElement[] {
  const text = node.textContent || "";
  node.textContent = "";
  return [...text].map((char) => {
    const span = document.createElement("span");
    span.className = "float-char";
    span.style.display = "inline-block";
    span.style.marginRight = "0.35em";
    if (char === " ") {
      span.textContent = "\u00A0";
    } else {
      span.textContent = char;
      span.style.willChange = "transform";
    }
    node.appendChild(span);
    return span;
  });
}

// --- Action ---

export function floatingLink(node: HTMLElement) {
  ensureMouseListener();

  const originalText = node.textContent || "";
  const chars = splitChars(node);

  // Build per-character animation state
  const states: CharState[] = [];
  chars.forEach((span) => {
    const orig =
      span.textContent === "\u00A0" ? " " : span.textContent || "";
    if (orig === " ") return;

    states.push({
      span,
      originalChar: orig,
      xChannels: makeChannels((rand(2, 4) | 0), 0.0002, 0.0011, 6, 32),
      yChannels: makeChannels((rand(2, 4) | 0), 0.0002, 0.0011, 6, 32),
      zChannels: makeChannels((rand(1, 3) | 0), 0.00012, 0.0007, 12, 55),
      rotXChannels: makeChannels((rand(1, 3) | 0), 0.00012, 0.0007, 4, 28),
      rotYChannels: makeChannels((rand(1, 3) | 0), 0.00012, 0.0007, 4, 28),
      rotZChannels: makeChannels((rand(1, 3) | 0), 0.0002, 0.0009, 2, 20),
      scaleChannel: {
        freq: rand(0.0002, 0.0006),
        phase: rand(0, Math.PI * 2),
        amp: rand(0.04, 0.18),
      },
      skewChannel: {
        freq: rand(0.0002, 0.0006),
        phase: rand(0, Math.PI * 2),
        amp: rand(2, 8),
      },
      calmPhase: rand(0, Math.PI * 2),
      lastGlitchCheck: 0,
      isGlitched: false,
      glitchChar: randomGlyph(),
      glitchInterval: rand(250, 800),
      glitchProb: rand(0.12, 0.4),
    });
  });

  // Register for wander + repulsion
  const initialX = parseFloat(node.style.left) || 50;
  const initialY = parseFloat(node.style.top) || 50;
  const entry: WanderEntry = {
    node,
    x: initialX,
    y: initialY,
    targetX: rand(5, 80),
    targetY: rand(8, 82),
  };
  wanderEntries.push(entry);

  // Main animation loop
  let raf: number;
  let prevTime = 0;

  function tick(time: number) {
    const dt = prevTime ? time - prevTime : 16;
    prevTime = time;

    // --- Wander with repulsion ---
    // Steer toward target
    entry.x += (entry.targetX - entry.x) * WANDER_LERP * dt;
    entry.y += (entry.targetY - entry.y) * WANDER_LERP * dt;

    // Repel from other links
    for (const other of wanderEntries) {
      if (other === entry) continue;
      const dx = entry.x - other.x;
      const dy = entry.y - other.y;
      const dist = Math.sqrt(dx * dx + dy * dy);
      if (dist < MIN_LINK_DIST && dist > 0.1) {
        const force = (MIN_LINK_DIST - dist) * 0.015;
        entry.x += (dx / dist) * force;
        entry.y += (dy / dist) * force;
      }
    }

    // Clamp to viewport
    entry.x = Math.max(5, Math.min(85, entry.x));
    entry.y = Math.max(8, Math.min(85, entry.y));

    // Pick new target when close
    const distToTarget = Math.sqrt(
      (entry.x - entry.targetX) ** 2 + (entry.y - entry.targetY) ** 2,
    );
    if (distToTarget < 3) {
      entry.targetX = rand(5, 80);
      entry.targetY = rand(8, 82);
    }

    node.style.left = `${entry.x}%`;
    node.style.top = `${entry.y}%`;

    // --- Per-character animation ---
    const isScrambling = node.closest(".is-animating") !== null;

    states.forEach((state) => {
      const rect = state.span.getBoundingClientRect();
      const cx = rect.left + rect.width / 2;
      const cy = rect.top + rect.height / 2;
      const dx = mouse.x - cx;
      const dy = mouse.y - cy;
      const dist = Math.sqrt(dx * dx + dy * dy);

      // 1 = far (wild), 0 = close (calm/reformed)
      const damping = smoothstep(INNER_RADIUS, OUTER_RADIUS, dist);

      // Wild state — layered sines per axis
      const wildX = evalChannels(state.xChannels, time);
      const wildY = evalChannels(state.yChannels, time);
      const wildZ = evalChannels(state.zChannels, time);
      const wildRotX = evalChannels(state.rotXChannels, time);
      const wildRotY = evalChannels(state.rotYChannels, time);
      const wildRotZ = evalChannels(state.rotZChannels, time);
      const wildScale =
        1 +
        Math.sin(time * state.scaleChannel.freq + state.scaleChannel.phase) *
          state.scaleChannel.amp;
      const wildSkew =
        Math.sin(time * state.skewChannel.freq + state.skewChannel.phase) *
        state.skewChannel.amp;

      // Calm state — very gentle residual drift
      const p = state.calmPhase;
      const calmX = Math.sin(time * 0.0004 + p) * 1.5;
      const calmY = Math.sin(time * 0.00035 + p + 1) * 1.5;
      const calmRotZ = Math.sin(time * 0.0003 + p + 2) * 0.8;
      const calmScale = 1 + Math.sin(time * 0.00025 + p + 3) * 0.015;

      // Blend wild → calm based on cursor proximity
      const x = calmX + (wildX - calmX) * damping;
      const y = calmY + (wildY - calmY) * damping;
      const z = wildZ * damping;
      const rotX = wildRotX * damping;
      const rotY = wildRotY * damping;
      const rotZ = calmRotZ + (wildRotZ - calmRotZ) * damping;
      const scale = calmScale + (wildScale - calmScale) * damping;
      const skew = wildSkew * damping;

      state.span.style.transform =
        `translate3d(${x}px, ${y}px, ${z}px) rotateX(${rotX}deg) rotateY(${rotY}deg) rotateZ(${rotZ}deg) scale(${scale}) skewX(${skew}deg)`;

      // Ambient glitch — per-character timing, skip during click scramble
      if (!isScrambling) {
        if (time - state.lastGlitchCheck > state.glitchInterval) {
          state.lastGlitchCheck = time;
          const shouldGlitch = Math.random() < damping * state.glitchProb;
          if (shouldGlitch && !state.isGlitched) {
            state.glitchChar = randomGlyph();
          }
          state.isGlitched = shouldGlitch;
        }
        state.span.textContent = state.isGlitched
          ? state.glitchChar
          : state.originalChar;
      }
    });

    raf = requestAnimationFrame(tick);
  }

  raf = requestAnimationFrame(tick);

  return {
    destroy() {
      cancelAnimationFrame(raf);
      const idx = wanderEntries.indexOf(entry);
      if (idx !== -1) wanderEntries.splice(idx, 1);
      node.textContent = originalText;
    },
  };
}
