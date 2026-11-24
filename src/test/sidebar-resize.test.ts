import type { PointerEvent as ReactPointerEvent } from "react";
import { describe, expect, it, vi } from "vitest";
import { handleSidebarResizeStart } from "../app/sidebar-resize";

function setupResizeDom(sidebarState: "collapsed" | "expanded") {
  document.body.innerHTML = `
    <main class="app-shell" data-sidebar="${sidebarState}">
      <aside class="sidebar"></aside>
      <div class="sidebar-resize-handle"></div>
      <div class="main-column">
        <div class="tab-bar"></div>
        <section class="main-panel"></section>
      </div>
    </main>
  `;
  return {
    shell: document.querySelector(".app-shell") as HTMLElement,
    handle: document.querySelector(".sidebar-resize-handle") as HTMLElement,
    mainColumn: document.querySelector(".main-column") as HTMLElement,
    tabBar: document.querySelector(".tab-bar") as HTMLElement,
    mainPanel: document.querySelector(".main-panel") as HTMLElement,
  };
}

function pointerEvent(type: string, clientX: number) {
  return new MouseEvent(type, { clientX }) as unknown as PointerEvent;
}

function reactPointerEvent(
  handle: HTMLElement,
  clientX: number,
): ReactPointerEvent<HTMLDivElement> {
  return {
    button: 0,
    clientX,
    currentTarget: handle,
    preventDefault: vi.fn(),
  } as unknown as ReactPointerEvent<HTMLDivElement>;
}

describe("handleSidebarResizeStart", () => {
  it("sets collapsed preview state as soon as a drag crosses the collapse threshold", () => {
    const { shell, handle } = setupResizeDom("expanded");
    const onStart = vi.fn();
    const onEnd = vi.fn();

    handleSidebarResizeStart(reactPointerEvent(handle, 240), 240, {
      collapseWidth: 160,
      minWidth: 188,
      maxWidth: () => 320,
      onStart,
      onEnd,
      commit: (fn) => fn(),
    });

    expect(onStart).toHaveBeenCalledOnce();
    expect(shell.dataset.sidebarPreview).toBe("expanded");

    window.dispatchEvent(pointerEvent("pointermove", 100));

    // The width var is the single source of truth — the card gutter, handle,
    // and tab strip all derive their offset from it in CSS.
    expect(shell.dataset.sidebarPreview).toBe("collapsed");
    expect(shell.style.getPropertyValue("--sidebar-w-current")).toBe("0px");

    window.dispatchEvent(pointerEvent("pointerup", 100));

    expect(onEnd).toHaveBeenCalledWith(0);
    expect(shell.dataset.sidebarPreview).toBeUndefined();
  });

  it("keeps an opening preview when dragging back out of collapsed width", () => {
    const { shell, handle } = setupResizeDom("collapsed");
    const onEnd = vi.fn();

    handleSidebarResizeStart(reactPointerEvent(handle, 0), 0, {
      collapseWidth: 160,
      minWidth: 188,
      maxWidth: () => 320,
      onStart: vi.fn(),
      onEnd,
      commit: (fn) => fn(),
    });

    expect(shell.dataset.sidebarPreview).toBe("collapsed");

    window.dispatchEvent(pointerEvent("pointermove", 220));

    expect(shell.dataset.sidebarPreview).toBe("opening");
    expect(shell.style.getPropertyValue("--sidebar-w-current")).toBe("220px");

    window.dispatchEvent(pointerEvent("pointermove", 240));

    expect(shell.dataset.sidebarPreview).toBe("opening");
    expect(shell.style.getPropertyValue("--sidebar-w-current")).toBe("240px");

    window.dispatchEvent(pointerEvent("pointerup", 220));

    expect(onEnd).toHaveBeenCalledWith(240);
    expect(shell.dataset.sidebarPreview).toBeUndefined();
  });

  it("keeps the snap tween alive (retargeting) instead of killing it on the next move", () => {
    const { shell, handle } = setupResizeDom("expanded");

    handleSidebarResizeStart(reactPointerEvent(handle, 240), 240, {
      collapseWidth: 160,
      minWidth: 188,
      maxWidth: () => 320,
      onStart: vi.fn(),
      onEnd: vi.fn(),
      commit: (fn) => fn(),
    });

    // Collapse, then drag straight back out past the min width.
    window.dispatchEvent(pointerEvent("pointermove", 100));
    window.dispatchEvent(pointerEvent("pointermove", 220));

    // The snap animates one value — the registered --sidebar-w-current length.
    // The grid, card gutter, handle and tab strip derive from it in CSS. They
    // share one clock and
    // can't drift apart mid-snap.
    const tween = "--sidebar-w-current var(--t-med) var(--ease-out)";
    expect(shell.style.transition).toBe(tween);

    // Further moves while the tween is in flight retarget it (the width var
    // updates) but must not reset the transition to "none" — that teleports
    // the sidebar from the interpolated width to the cursor in one frame.
    window.dispatchEvent(pointerEvent("pointermove", 260));
    expect(shell.style.transition).toBe(tween);
    expect(shell.style.getPropertyValue("--sidebar-w-current")).toBe("260px");

    // Once the width tween completes, tracking goes back to transition-less.
    const end = new Event("transitionend") as TransitionEvent;
    Object.defineProperty(end, "propertyName", {
      value: "--sidebar-w-current",
    });
    shell.dispatchEvent(end);
    expect(shell.style.transition).toBe("none");

    window.dispatchEvent(pointerEvent("pointerup", 260));
    expect(shell.style.transition).toBe("");
  });

  it("flushes the snap transition before reopening after a held off-window collapse", () => {
    const { shell, handle } = setupResizeDom("expanded");

    handleSidebarResizeStart(reactPointerEvent(handle, 240), 240, {
      collapseWidth: 160,
      minWidth: 188,
      maxWidth: () => 320,
      onStart: vi.fn(),
      onEnd: vi.fn(),
      commit: (fn) => fn(),
    });

    window.dispatchEvent(pointerEvent("pointermove", -80));
    const end = new Event("transitionend") as TransitionEvent;
    Object.defineProperty(end, "propertyName", {
      value: "--sidebar-w-current",
    });
    shell.dispatchEvent(end);
    expect(shell.dataset.sidebarPreview).toBe("collapsed");
    expect(shell.style.transition).toBe("none");

    // The held-collapse cleared the transition; reopening must re-arm it and
    // flush (read offsetWidth) BEFORE flipping preview/width, so the first
    // reopening frame interpolates from the current width instead of jumping.
    const flushes: string[] = [];
    Object.defineProperty(shell, "offsetWidth", {
      configurable: true,
      get() {
        flushes.push([shell.style.transition, shell.dataset.sidebarPreview].join("|"));
        return 0;
      },
    });

    window.dispatchEvent(pointerEvent("pointermove", 170));

    expect(flushes).toEqual([
      ["--sidebar-w-current var(--t-med) var(--ease-out)", "collapsed"].join("|"),
    ]);
    expect(shell.dataset.sidebarPreview).toBe("opening");
    expect(shell.style.getPropertyValue("--sidebar-w-current")).toBe("188px");

    window.dispatchEvent(pointerEvent("pointerup", 170));
  });
});
