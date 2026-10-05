import { useEffect, useState } from "react";
import { onActivityTimelineUpdated } from "../../lib/activity-timeline";
import {
  type DayIntelligenceDto,
  dayIntelligence,
  onDayIntelligenceUpdated,
} from "../../lib/day-intelligence";

export type DayIntelligenceState = {
  /** The read for `day`, or null while it is loading. */
  data: DayIntelligenceDto | null;
  loaded: boolean;
  error: string | null;
};

/**
 * The day's workstreams and reports for the "Today" grid, read again when
 * the backend reports new intelligence for that day or new captured
 * activity. A read for a day the view has left is dropped.
 */
export function useDayIntelligence(day: string): DayIntelligenceState {
  const [state, setState] = useState<{ day: string } & DayIntelligenceState>({
    day,
    data: null,
    loaded: false,
    error: null,
  });

  useEffect(() => {
    let active = true;
    let latest = 0;
    const load = async () => {
      const request = ++latest;
      try {
        const data = await dayIntelligence(day);
        if (active && request === latest) {
          setState({ day, data: data ?? null, loaded: true, error: null });
        }
      } catch (failure) {
        if (active && request === latest) {
          const message =
            failure && typeof failure === "object" && "message" in failure
              ? String(failure.message)
              : String(failure);
          setState((previous) => ({
            day,
            data: previous.day === day ? previous.data : null,
            loaded: true,
            error: message,
          }));
        }
      }
    };
    void load();
    const cleanups: (() => void)[] = [];
    void onDayIntelligenceUpdated((payload) => {
      if (payload.day === day) void load();
    }).then((unlisten) => (active ? cleanups.push(unlisten) : unlisten()));
    void onActivityTimelineUpdated(() => void load()).then((unlisten) =>
      active ? cleanups.push(unlisten) : unlisten(),
    );
    return () => {
      active = false;
      for (const cleanup of cleanups) cleanup();
    };
  }, [day]);

  return state.day === day
    ? { data: state.data, loaded: state.loaded, error: state.error }
    : { data: null, loaded: false, error: null };
}
