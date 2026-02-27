import { gsap } from "gsap";

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

function pick<T>(arr: T[]): T {
  return arr[Math.floor(Math.random() * arr.length)];
}

const INNER_RADIUS = 40;
const OUTER_RADIUS = 200;

function splitChars(node: HTMLElement): HTMLSpanElement[] {
  const text = node.textContent || "";
  node.textContent = "";
  return [...text].map((char) => {
    const span = document.createElement("span");
    span.className = "float-char";
    span.style.display = "inline-block";
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

export function floatingLink(node: HTMLElement) {
  ensureMouseListener();

  const originalText = node.textContent || "";
  const chars = splitChars(node);
  const animatable = chars.filter((c) => c.textContent !== "\u00A0");

  const allTweens: gsap.core.Tween[] = [];

  animatable.forEach((char) => {
    // Layer 1: translation drift — slow, wide orbits
    allTweens.push(
      gsap.to(char, {
        x: rand(12, 45) * randSign(),
        y: rand(12, 45) * randSign(),
        z: rand(25, 80) * randSign(),
        duration: rand(3, 10),
        ease: pick(["sine.inOut", "power1.inOut", "power2.inOut"]),
        repeat: -1,
        yoyo: true,
        delay: rand(0, 5),
      }),
    );

    // Layer 2: rotation + skew — medium speed, different axes emphasized
    allTweens.push(
      gsap.to(char, {
        rotateX: rand(10, 40) * randSign(),
        rotateY: rand(10, 40) * randSign(),
        rotateZ: rand(5, 30) * randSign(),
        skewX: rand(2, 12) * randSign(),
        duration: rand(2, 7),
        ease: pick([
          "sine.inOut",
          "elastic.inOut(1, 0.3)",
          "back.inOut(1.7)",
        ]),
        repeat: -1,
        yoyo: true,
        delay: rand(0, 4),
      }),
    );

    // Layer 3: scale + opacity
    allTweens.push(
      gsap.to(char, {
        scale: rand(0.8, 1.25),
        opacity: rand(0.6, 1),
        duration: rand(1.5, 5),
        ease: "sine.inOut",
        repeat: -1,
        yoyo: true,
        delay: rand(0, 3),
      }),
    );
  });

  // Cursor proximity damping — modulate timeScale per character
  let raf: number;

  function tick() {
    animatable.forEach((char) => {
      const rect = char.getBoundingClientRect();
      const cx = rect.left + rect.width / 2;
      const cy = rect.top + rect.height / 2;
      const dx = mouse.x - cx;
      const dy = mouse.y - cy;
      const dist = Math.sqrt(dx * dx + dy * dy);
      const damping = smoothstep(INNER_RADIUS, OUTER_RADIUS, dist);

      gsap.getTweensOf(char).forEach((t) => t.timeScale(damping));
    });
    raf = requestAnimationFrame(tick);
  }

  raf = requestAnimationFrame(tick);

  return {
    destroy() {
      cancelAnimationFrame(raf);
      allTweens.forEach((t) => t.kill());
      node.textContent = originalText;
    },
  };
}
