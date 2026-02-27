const GLYPHS = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%&*+=?/";
const MIN_DURATION = 800;
const POST_RESOLVE_PAUSE = 250;
const FLIP_INTERVAL = 40; // ms between glyph changes — mechanical tick rate

function randomGlyph(): string {
  return GLYPHS[Math.floor(Math.random() * GLYPHS.length)];
}

export function scramble(
  el: HTMLElement,
  finalText: string,
  onComplete: () => void,
  duration?: number
): void {
  const chars = finalText.split("");
  const total = Math.max(duration ?? chars.length * 140, MIN_DURATION);

  // Each character gets a resolve time — left-to-right wave with jitter
  const resolveTimes = chars.map((_, i) => {
    const base = (i / chars.length) * total;
    const jitter = (Math.random() - 0.3) * 120;
    return Math.max(60, base + jitter);
  });

  const start = performance.now();
  let lastFlip = 0;

  function tick(now: number) {
    const elapsed = now - start;

    // Only update display at flip intervals for that mechanical feel
    if (now - lastFlip < FLIP_INTERVAL) {
      if (elapsed < total) {
        requestAnimationFrame(tick);
      }
      return;
    }
    lastFlip = now;

    let allResolved = true;
    const display = chars.map((char, i) => {
      if (char === " ") return " ";
      if (elapsed >= resolveTimes[i]) return char;
      allResolved = false;
      return randomGlyph();
    });

    el.textContent = display.join("");

    if (allResolved || elapsed >= total) {
      el.textContent = finalText;
      setTimeout(onComplete, POST_RESOLVE_PAUSE);
    } else {
      requestAnimationFrame(tick);
    }
  }

  // Kick off with a scrambled state immediately
  el.textContent = chars.map((c) => (c === " " ? " " : randomGlyph())).join("");
  requestAnimationFrame(tick);
}

// Subtle hover glitch — swaps a single random character briefly, then restores it
const HOVER_INTERVAL = 120; // ms between glitch ticks
const GLITCH_CHANCE = 0.4; // probability a tick actually glitches

export function startHoverGlitch(el: HTMLElement, text: string): () => void {
  let frame: number | null = null;
  let lastTick = 0;

  function tick(now: number) {
    if (now - lastTick >= HOVER_INTERVAL) {
      lastTick = now;

      if (Math.random() < GLITCH_CHANCE) {
        const chars = text.split("");
        // Pick one random non-space character to corrupt
        const candidates = chars
          .map((c, i) => (c !== " " ? i : -1))
          .filter((i) => i !== -1);
        const idx = candidates[Math.floor(Math.random() * candidates.length)];
        chars[idx] = randomGlyph();
        el.textContent = chars.join("");
      } else {
        el.textContent = text;
      }
    }

    frame = requestAnimationFrame(tick);
  }

  frame = requestAnimationFrame(tick);

  // Return cleanup function
  return () => {
    if (frame !== null) cancelAnimationFrame(frame);
    el.textContent = text;
  };
}
