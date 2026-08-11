type TransitionConfig = {
  duration: number;
  css: (t: number, u: number) => string;
};

function prefersReducedMotion(): boolean {
  return typeof window !== "undefined"
    && typeof window.matchMedia === "function"
    && window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}

export function modalBackdrop(_node: Element): TransitionConfig {
  return {
    duration: prefersReducedMotion() ? 1 : 140,
    css: (t) => `opacity:${t};`,
  };
}

export function modalPanel(_node: Element): TransitionConfig {
  return {
    duration: prefersReducedMotion() ? 1 : 180,
    css: (t, u) => [
      `opacity:${t}`,
      `transform:translateY(${u * 10}px) scale(${0.98 + t * 0.02})`,
    ].join(";"),
  };
}
