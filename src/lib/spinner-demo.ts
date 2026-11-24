// Dev-only coordinator for inspecting Clovy's shared spinner in representative
// production contexts from one console command:
//
//   window.__spinnerDemo()       show the loading toast sample
//   window.__spinnerDemo(false)  clear the sample
//
// The individual surfaces keep owning their own preview state. This module
// composes those existing drivers so it never starts a real request, mutates
// user data, or creates a second presentation-only version of a spinner.

import { toast } from "../components/ui/Toaster";
type SpinnerDemoWindow = Window & {
  __spinnerDemo?: (show?: boolean) => string;
};

export type SpinnerDemoApi = {
  dispose: () => void;
};

export function registerSpinnerDemo(): SpinnerDemoApi {
  const target = window as SpinnerDemoWindow;
  let active = false;
  let loadingToastId: string | number | undefined;

  function clear() {
    if (loadingToastId !== undefined) {
      toast.dismiss(loadingToastId);
      loadingToastId = undefined;
    }
    active = false;
  }

  const run = (show: boolean = true) => {
    if (!show) {
      clear();
      return "Spinner demo cleared.";
    }

    active = true;
    loadingToastId ??= toast.loading("Spinner showcase");

    return "Loading toast shown. Run __spinnerDemo(false) to clear.";
  };

  target.__spinnerDemo = run;

  return {
    dispose() {
      if (active) clear();
      if (target.__spinnerDemo === run) delete target.__spinnerDemo;
    },
  };
}
