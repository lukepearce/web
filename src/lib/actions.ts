import { scramble, startHoverGlitch } from "../scramble";

type HoverGlitchParam = string | { text: string; enabled: boolean };

export function hoverGlitch(node: HTMLElement, param: HoverGlitchParam) {
  let text = typeof param === "string" ? param : param.text;
  let enabled = typeof param === "string" ? true : param.enabled;
  let stop: (() => void) | null = null;

  // Start glitching immediately
  if (enabled) {
    stop = startHoverGlitch(node, text);
  }

  function onEnter() {
    // Hover = calm, stop glitching
    if (stop) {
      stop();
      stop = null;
    }
  }

  function onLeave() {
    // Leave = resume glitching
    if (enabled) {
      stop = startHoverGlitch(node, text);
    }
  }

  node.addEventListener("mouseenter", onEnter);
  node.addEventListener("mouseleave", onLeave);

  return {
    update(param: HoverGlitchParam) {
      text = typeof param === "string" ? param : param.text;
      const nowEnabled = typeof param === "string" ? true : param.enabled;

      if (enabled && !nowEnabled && stop) {
        stop();
        stop = null;
      }
      if (!enabled && nowEnabled && !stop) {
        stop = startHoverGlitch(node, text);
      }

      enabled = nowEnabled;
    },
    destroy() {
      if (stop) stop();
      node.removeEventListener("mouseenter", onEnter);
      node.removeEventListener("mouseleave", onLeave);
    },
  };
}

interface ClickScrambleParam {
  text: string;
  url: string;
  duration?: number;
  onStart: () => void;
  onEnd: () => void;
  disabled: boolean;
}

export function clickScramble(node: HTMLElement, param: ClickScrambleParam) {
  let current = param;

  function onClick(e: MouseEvent) {
    if (current.disabled) {
      e.preventDefault();
      return;
    }

    if (!current.url.startsWith("http")) return;

    e.preventDefault();
    current.onStart();

    scramble(
      node,
      current.text,
      () => {
        current.onEnd();
        window.location.href = current.url;
      },
      current.duration,
    );
  }

  node.addEventListener("click", onClick);

  return {
    update(param: ClickScrambleParam) {
      current = param;
    },
    destroy() {
      node.removeEventListener("click", onClick);
    },
  };
}
