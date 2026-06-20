import { describe, expect, it } from "vitest";
import { applyOptimisticMove, type Task } from "./tasks";

// Minimal Task shaped for the move logic — only id/key/column_id/order matter.
function t(id: string, column_id: string, order_in_column: number): Task {
  return { id, key: id, column_id, order_in_column } as unknown as Task;
}

const board = () => [
  t("a", "todo", 1024),
  t("b", "todo", 2048),
  t("c", "doing", 1024),
];

describe("applyOptimisticMove", () => {
  it("returns the list unchanged when the task key is unknown", () => {
    const list = board();
    expect(applyOptimisticMove(list, { taskKey: "zzz", column_id: "doing" })).toBe(list);
  });

  it("appends past the last sibling when there are no position hints", () => {
    const moved = applyOptimisticMove(board(), { taskKey: "a", column_id: "doing" });
    const a = moved.find((x) => x.id === "a")!;
    expect(a.column_id).toBe("doing");
    // doing's last order is 1024 → appended at 1024 + 1024.
    expect(a.order_in_column).toBe(2048);
  });

  it("drops before a sibling (top of column) using a 1024 gap", () => {
    const moved = applyOptimisticMove(board(), {
      taskKey: "a",
      column_id: "doing",
      before_task_id: "c",
    });
    const a = moved.find((x) => x.id === "a")!;
    expect(a.order_in_column).toBe(0); // 1024 - 1024
  });

  it("drops after a sibling (bottom of column) using a 1024 gap", () => {
    const moved = applyOptimisticMove(board(), {
      taskKey: "a",
      column_id: "doing",
      after_task_id: "c",
    });
    const a = moved.find((x) => x.id === "a")!;
    expect(a.order_in_column).toBe(2048); // 1024 + 1024
  });

  it("bisects between two siblings when dropping before the second", () => {
    const list = [t("a", "todo", 1024), t("b", "todo", 2048), t("c", "todo", 3072)];
    const moved = applyOptimisticMove(list, {
      taskKey: "c",
      column_id: "todo",
      before_task_id: "b",
    });
    const c = moved.find((x) => x.id === "c")!;
    // between a(1024) and b(2048).
    expect(c.order_in_column).toBe(1536);
  });
});
