import type { ReactNode } from "react";
import type { MessageKey } from "../../i18n";

export type TimelineLaneProps = {
  from: string;
  to: string;
  dayStartMs: number;
  dayEndMs: number;
  /** Returns a 0..1 fraction of the day for a timestamp in milliseconds. */
  xForTime(ms: number): number;
};

/**
 * Extension point for custom timeline lanes rendered on the same 24-hour scale.
 * The primary sessions lane renders first, followed by each registered extra lane.
 */
export type TimelineLane = {
  id: string;
  label: MessageKey | string;
  render: (props: TimelineLaneProps) => ReactNode;
};

/**
 * Registry for additional timeline lanes (e.g. coding-agent lane).
 */
export const EXTRA_TIMELINE_LANES: TimelineLane[] = [];
