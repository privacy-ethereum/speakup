// The SpeakUp VM animation: a self-contained HTML/CSS snippet (markup + inline
// styles + a <style> block of keyframes) inlined at build time. The snippet's
// stage is a fixed 1096px-wide board scaled to its container; the snippet ships
// a fit <script>, but innerHTML-injected scripts are inert, so the scaling is
// re-driven here instead.

import snippet from "./zkvm-embed-snippet.html?raw";

const STAGE_WIDTH = 375; // the snippet's intrinsic stage width, in px

/// Inject the animation into `container` and keep its stage scaled to fit.
export const mountVmEmbed = (container: HTMLElement): void => {
  container.innerHTML = snippet.replace(/<script[\s\S]*?<\/script>/i, "");
  const box = container.querySelector<HTMLElement>(".zkvm-embed");
  const stage = box?.querySelector<HTMLElement>(".zkvm-stage");
  if (!box || !stage) return;

  const fit = () => {
    const s = box.clientWidth / STAGE_WIDTH;
    stage.style.setProperty("--zkv-s", String(s));
    box.style.height = `${stage.offsetHeight * s}px`;
  };
  fit();
  new ResizeObserver(fit).observe(box);
};
