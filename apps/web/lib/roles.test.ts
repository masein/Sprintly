import { describe, expect, it } from "vitest";
import { canEditTasks, canManageProject } from "./roles";

describe("canEditTasks", () => {
  it("is leads and contributors", () => {
    expect(canEditTasks({ your_role: "lead" })).toBe(true);
    expect(canEditTasks({ your_role: "contributor" }, "member")).toBe(true);
    expect(canEditTasks({ your_role: "watcher" })).toBe(false);
    expect(canEditTasks({ your_role: null })).toBe(false);
  });

  it("lets admins in and keeps viewers out", () => {
    expect(canEditTasks({ your_role: null }, "admin")).toBe(true);
    expect(canEditTasks({ your_role: "contributor" }, "viewer")).toBe(false);
  });

  it("treats an archived project as read-only", () => {
    expect(canEditTasks({ your_role: "lead", archived_at: "2026-10-01T00:00:00Z" })).toBe(false);
    expect(canEditTasks({ your_role: null, archived_at: "2026-10-01T00:00:00Z" }, "admin")).toBe(false);
  });

  it("says no before the project has loaded", () => {
    expect(canEditTasks(undefined)).toBe(false);
  });
});

describe("canManageProject", () => {
  it("is leads and admins only", () => {
    expect(canManageProject({ your_role: "lead" })).toBe(true);
    expect(canManageProject({ your_role: "contributor" })).toBe(false);
    expect(canManageProject({ your_role: null }, "admin")).toBe(true);
  });
});
