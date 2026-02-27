import { scramble, startHoverGlitch } from "../scramble";

type HoverGlitchParam = string | { text: string; enabled: boolean };

export function hoverGlitch(node: HTMLElement, param: HoverGlitchParam) {
  let text = typeof param === "string" ? param : param.text;
  let enabled = typeof param === "string" ? true : param.enabled;
  let stop: (() => void) | null = null;

  function onEnter() {
    if (!enabled) return;
    stop = startHoverGlitch(node, text);
  }

  function onLeave() {
    if (stop) {
      stop();
      stop = null;
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
