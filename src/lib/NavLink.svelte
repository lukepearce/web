<script lang="ts">
  import type { LinkEntry } from "../links";
  import { clickScramble } from "./actions";
  import { floatingLink } from "./float";

  let {
    link,
    animating,
    homeX,
    homeY,
    onAnimateStart,
    onAnimateEnd,
  }: {
    link: LinkEntry;
    animating: boolean;
    homeX: number;
    homeY: number;
    onAnimateStart: () => void;
    onAnimateEnd: () => void;
  } = $props();
</script>

<a
  href={link.url}
  class="absolute text-[#c8c8c8] no-underline text-base w-fit"
  style="left: {homeX}%; top: {homeY}%; transform-style: preserve-3d;"
  use:floatingLink
  use:clickScramble={{
    text: link.label,
    url: link.url,
    duration: link.scrambleDuration,
    onStart: onAnimateStart,
    onEnd: onAnimateEnd,
    disabled: animating,
  }}
>
  {link.label}
</a>
