import { useLayoutEffect, useState } from "react";
import { FONT_SCALE_PRESETS, useFontScaleId } from "../../lib/font-scale";

/** Observe only the scroller, never the canvas whose height the layout sets. */
export function useCalendarViewport() {
  const [element, ref] = useState<HTMLDivElement | null>(null);
  const [size, setSize] = useState({ viewportHeight: 0, minCardHeight: 22, cardGap: 6 });
  const fontScale = useFontScaleId();

  useLayoutEffect(() => {
    if (!element) return;
    const measure = () => {
      const style = getComputedStyle(element);
      const viewportHeight = Math.max(
        0,
        element.clientHeight -
          (Number.parseFloat(style.paddingTop) || 0) -
          (Number.parseFloat(style.paddingBottom) || 0),
      );
      const minCardHeight = Math.ceil(
        Math.max(
          Number.parseFloat(style.getPropertyValue("--control-xs")) || 22,
          (Number.parseFloat(style.fontSize) ||
            13 * (FONT_SCALE_PRESETS.find((preset) => preset.id === fontScale)?.value ?? 1)) *
            1.25 +
            2,
        ),
      );
      const cardGap = Number.parseFloat(style.getPropertyValue("--sp-2")) || 6;
      setSize((previous) =>
        previous.viewportHeight === viewportHeight &&
        previous.minCardHeight === minCardHeight &&
        previous.cardGap === cardGap
          ? previous
          : { viewportHeight, minCardHeight, cardGap },
      );
    };
    // Font preference changes require a new line-box measurement even if the
    // scroller's outer dimensions stay unchanged.
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [element, fontScale]);

  return { ref, ...size };
}
