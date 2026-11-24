import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { registerSpinnerDemo } from "../lib/spinner-demo";

const mocks = vi.hoisted(() => ({
  dismissToast: vi.fn(),
  showLoadingToast: vi.fn(() => "spinner-demo-toast"),
}));

vi.mock("../components/ui/Toaster", () => ({
  toast: {
    dismiss: mocks.dismissToast,
    loading: mocks.showLoadingToast,
  },
}));

type SpinnerDemoWindow = Window & {
  __spinnerDemo?: (show?: boolean) => string;
};

describe("spinner demo", () => {
  const win = window as SpinnerDemoWindow;
  let dispose: (() => void) | undefined;

  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    dispose?.();
    dispose = undefined;
    delete win.__spinnerDemo;
  });

  it("shows and clears the loading toast", () => {
    ({ dispose } = registerSpinnerDemo());

    expect(win.__spinnerDemo).toBeTypeOf("function");
    expect(win.__spinnerDemo?.()).toBe("Loading toast shown. Run __spinnerDemo(false) to clear.");
    expect(mocks.showLoadingToast).toHaveBeenCalledWith("Spinner showcase");

    expect(win.__spinnerDemo?.(false)).toBe("Spinner demo cleared.");
    expect(mocks.dismissToast).toHaveBeenCalledWith("spinner-demo-toast");
  });

  it("does not create duplicate loading toasts when shown twice", () => {
    ({ dispose } = registerSpinnerDemo());

    win.__spinnerDemo?.();
    win.__spinnerDemo?.();

    expect(mocks.showLoadingToast).toHaveBeenCalledOnce();
  });

  it("removes its console hook and clears active demo state on disposal", () => {
    ({ dispose } = registerSpinnerDemo());
    win.__spinnerDemo?.();

    dispose();
    dispose = undefined;

    expect(win.__spinnerDemo).toBeUndefined();
    expect(mocks.dismissToast).toHaveBeenCalledWith("spinner-demo-toast");
  });
});
