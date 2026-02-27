import { links } from "./links";
import { scramble, startHoverGlitch } from "./scramble";
import "./style.css";

let animating = false;

function createPage() {
  const app = document.querySelector<HTMLDivElement>("#app")!;

  const header = document.createElement("h1");
  header.textContent = "Luke Pearce";
  let stopHeaderGlitch: (() => void) | null = null;
  header.addEventListener("mouseenter", () => {
    stopHeaderGlitch = startHoverGlitch(header, "Luke Pearce");
  });
  header.addEventListener("mouseleave", () => {
    if (stopHeaderGlitch) {
      stopHeaderGlitch();
      stopHeaderGlitch = null;
    }
  });
  app.appendChild(header);

  const nav = document.createElement("nav");

  for (const link of links) {
    const a = document.createElement("a");
    a.href = link.url;
    a.textContent = link.label;

    let stopGlitch: (() => void) | null = null;

    a.addEventListener("mouseenter", () => {
      if (animating) return;
      stopGlitch = startHoverGlitch(a, link.label);
    });

    a.addEventListener("mouseleave", () => {
      if (stopGlitch) {
        stopGlitch();
        stopGlitch = null;
      }
    });

    a.addEventListener("click", (e) => {
      if (animating) {
        e.preventDefault();
        return;
      }

      // Let mailto: and other non-http links through without scramble
      if (!link.url.startsWith("http")) return;

      e.preventDefault();
      if (stopGlitch) {
        stopGlitch();
        stopGlitch = null;
      }
      animating = true;
      nav.classList.add("is-animating");

      scramble(a, link.label, () => {
        animating = false;
        nav.classList.remove("is-animating");
        window.location.href = link.url;
      }, link.scrambleDuration);
    });

    nav.appendChild(a);
  }

  app.appendChild(nav);
}

createPage();
