import { beforeEach, describe, expect, it } from "vitest";
import {
  forgetClovyHomeStoredSessionId,
  clovyHomeProfileRemovalPlan,
  readClovyHomeStoredSessionId,
  reconcileClovyHomeProfileRemoval,
  writeClovyHomeStoredSessionId,
} from "../lib/clovy-home";

describe("Clovy Home", () => {
  beforeEach(() => window.localStorage.clear());

  it("stores one Home session per profile", () => {
    writeClovyHomeStoredSessionId("default", "session-default");
    writeClovyHomeStoredSessionId("work", "session-work");

    expect(readClovyHomeStoredSessionId("default")).toBe("session-default");
    expect(readClovyHomeStoredSessionId("work")).toBe("session-work");

    forgetClovyHomeStoredSessionId("work", "another-session");
    expect(readClovyHomeStoredSessionId("work")).toBe("session-work");
    forgetClovyHomeStoredSessionId("work", "session-work");
    expect(readClovyHomeStoredSessionId("work")).toBeUndefined();
  });

  it("moves a removed profile's Home thread into the existing default thread", () => {
    writeClovyHomeStoredSessionId("default", "home-default");
    writeClovyHomeStoredSessionId("research", "home-research");
    window.localStorage.setItem(
      "clovy:home:direct-turns:v1",
      JSON.stringify({
        "home-default": [{ id: "default-turn", createdAt: "2026-07-24T09:00:00Z" }],
        "home-research": [{ id: "research-turn", createdAt: "2026-07-24T10:00:00Z" }],
      }),
    );
    window.localStorage.setItem(
      "clovy:home:task-handoffs:v1",
      JSON.stringify({
        "home-default": [{ id: "default-task" }],
        "home-research": [{ id: "research-task" }],
      }),
    );
    window.localStorage.setItem(
      "clovy:home:check-ins:v1",
      JSON.stringify({
        default: { date: "2026-07-24", createdAt: "2026-07-24T09:00:00Z" },
        research: { date: "2026-07-24", createdAt: "2026-07-24T10:00:00Z" },
      }),
    );

    expect(clovyHomeProfileRemovalPlan("research", "move")).toEqual({
      sourceSessionId: "home-research",
      targetSessionId: "home-default",
      redundantSessionId: "home-research",
    });
    reconcileClovyHomeProfileRemoval("research", "move");

    expect(readClovyHomeStoredSessionId("research")).toBeUndefined();
    expect(readClovyHomeStoredSessionId("default")).toBe("home-default");
    const turns = JSON.parse(
      window.localStorage.getItem("clovy:home:direct-turns:v1") ?? "{}",
    ) as Record<string, Array<{ id: string }>>;
    expect(turns["home-default"].map((turn) => turn.id)).toEqual(["default-turn", "research-turn"]);
    expect(turns).not.toHaveProperty("home-research");
    const handoffs = JSON.parse(
      window.localStorage.getItem("clovy:home:task-handoffs:v1") ?? "{}",
    ) as Record<string, Array<{ id: string }>>;
    expect(handoffs["home-default"].map((handoff) => handoff.id)).toEqual([
      "default-task",
      "research-task",
    ]);
    expect(handoffs).not.toHaveProperty("home-research");
    const checkIns = JSON.parse(
      window.localStorage.getItem("clovy:home:check-ins:v1") ?? "{}",
    ) as Record<string, unknown>;
    expect(checkIns).toHaveProperty("default");
    expect(checkIns).not.toHaveProperty("research");
  });

  it("purges current and legacy Home history when a profile is deleted permanently", () => {
    writeClovyHomeStoredSessionId("research", "home-research");
    for (const key of [
      "clovy:home:direct-turns:v1",
      "clovy:home:task-handoffs:v1",
      "clovy.home.directTurns.v1",
      "clovy.home.taskHandoffs.v1",
    ]) {
      window.localStorage.setItem(
        key,
        JSON.stringify({ "home-research": [{ id: `${key}:private` }] }),
      );
    }

    reconcileClovyHomeProfileRemoval("research", "delete");

    expect(readClovyHomeStoredSessionId("research")).toBeUndefined();
    for (const key of [
      "clovy:home:direct-turns:v1",
      "clovy:home:task-handoffs:v1",
      "clovy.home.directTurns.v1",
      "clovy.home.taskHandoffs.v1",
    ]) {
      expect(JSON.parse(window.localStorage.getItem(key) ?? "{}")).not.toHaveProperty(
        "home-research",
      );
    }
  });
});
