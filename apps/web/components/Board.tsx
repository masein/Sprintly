"use client";

// The Kanban board.
//
// What works in M3 phase A:
//   • Renders columns horizontally with their task cards.
//   • Drag-reorder columns (mouse + keyboard).
//   • Drag tasks within and across columns. Optimistic update; server is
//     authoritative and the cache reconciles via WS (task_moved) or refetch
//     onSettled.
//   • Inline "add card" at the bottom of each column.
//   • Edit/delete columns via per-column menu.
//
// What's deliberately missing:
//   • Realtime presence dots — Phase B.
//   • Filter chips on the board header — Phase C.

import { useMemo, useState } from "react";
import {
  DndContext,
  DragOverlay,
  KeyboardSensor,
  PointerSensor,
  closestCenter,
  useDroppable,
  useSensor,
  useSensors,
  type DragEndEvent,
  type DragStartEvent,
} from "@dnd-kit/core";
import {
  SortableContext,
  horizontalListSortingStrategy,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
  arrayMove,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { GripVertical, MoreHorizontal, Plus, Trash2, X, Check } from "lucide-react";
import {
  createColumn,
  deleteColumn,
  editColumn,
  reorderColumns,
  type Board as BoardModel,
  type Column,
} from "@/lib/projects";
import { useCreateTask, useMoveTask, useTasks, type Task } from "@/lib/tasks";
import { TaskCard } from "./TaskCard";
import { BoardFilters, toFilterDSL, type Chip } from "./BoardFilters";

export function Board({
  projectKey,
  projectId,
  board,
  canManage,
  onBoardChange,
}: {
  projectKey: string;
  projectId: string;
  board: BoardModel;
  canManage: boolean;
  onBoardChange: (next: BoardModel) => void;
}) {
  const [error, setError] = useState<string | null>(null);
  const [chips, setChips] = useState<Chip[]>([]);
  const filter = chips.length > 0 ? toFilterDSL(chips) : undefined;
  const { data: tasks = [] } = useTasks(projectKey, projectId, filter);
  const move = useMoveTask(projectId);

  const tasksByColumn = useMemo(() => {
    const m = new Map<string, Task[]>();
    for (const c of board.columns) m.set(c.id, []);
    for (const t of tasks) {
      const list = m.get(t.column_id);
      if (list) list.push(t);
    }
    for (const list of m.values()) list.sort((a, b) => a.order_in_column - b.order_in_column);
    return m;
  }, [board.columns, tasks]);

  const [activeTask, setActiveTask] = useState<Task | null>(null);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  function onDragStart(e: DragStartEvent) {
    const id = String(e.active.id);
    const t = tasks.find((x) => x.id === id);
    if (t) setActiveTask(t);
  }

  async function onDragEnd(e: DragEndEvent) {
    const { active, over } = e;
    setActiveTask(null);
    if (!over) return;

    const activeKind = active.data.current?.kind as "task" | "column" | undefined;
    if (activeKind === "column") {
      if (active.id === over.id) return;
      const oldIndex = board.columns.findIndex((c) => c.id === active.id);
      const newIndex = board.columns.findIndex((c) => c.id === over.id);
      if (oldIndex < 0 || newIndex < 0) return;
      const next = arrayMove(board.columns, oldIndex, newIndex);
      onBoardChange({ ...board, columns: next });
      try {
        await reorderColumns(board.id, next.map((c) => c.id));
      } catch {
        onBoardChange({ ...board, columns: board.columns });
        setError("Column reorder rejected.");
      }
      return;
    }

    // Task move. The destination is either another task (drop on a card) or
    // a column (drop on the column body).
    const movingTask = tasks.find((t) => t.id === active.id);
    if (!movingTask) return;

    const overId = String(over.id);
    const overTask = tasks.find((t) => t.id === overId);
    // Column-body drop zones use a `:body` suffix on the column id.
    const bodyColumnId = overId.endsWith(":body")
      ? overId.slice(0, -":body".length)
      : null;
    const overColumn = bodyColumnId
      ? board.columns.find((c) => c.id === bodyColumnId)
      : null;

    let payload: Parameters<typeof move.mutate>[0] | null = null;
    if (overTask) {
      // Dropped on a sibling card. If the same card, no-op.
      if (overTask.id === movingTask.id) return;
      payload = {
        taskKey: movingTask.key,
        column_id: overTask.column_id,
        before_task_id: overTask.id,
      };
    } else if (overColumn) {
      // Empty column or the bottom-of-column drop zone.
      payload = { taskKey: movingTask.key, column_id: overColumn.id };
    }

    if (payload) move.mutate(payload);
  }

  return (
    <div>
      <BoardFilters chips={chips} onChange={setChips} />
      {error && (
        <div className="mono mb-3 rounded border border-red-500/30 bg-red-500/10 p-2 text-xs text-red-200">
          {error}
        </div>
      )}
      <DndContext
        sensors={sensors}
        collisionDetection={closestCenter}
        onDragStart={onDragStart}
        onDragEnd={onDragEnd}
      >
        <div className="flex items-start gap-3 overflow-x-auto pb-4">
          <SortableContext
            items={board.columns.map((c) => c.id)}
            strategy={horizontalListSortingStrategy}
            disabled={!canManage}
          >
            {board.columns.map((col) => (
              <ColumnView
                key={col.id}
                projectKey={projectKey}
                projectId={projectId}
                column={col}
                tasks={tasksByColumn.get(col.id) ?? []}
                canManage={canManage}
                onEdit={async (patch) => {
                  const updated = await editColumn(col.id, patch);
                  onBoardChange({
                    ...board,
                    columns: board.columns.map((c) =>
                      c.id === col.id ? updated : c,
                    ),
                  });
                }}
                onDelete={async () => {
                  try {
                    await deleteColumn(col.id);
                    onBoardChange({
                      ...board,
                      columns: board.columns.filter((c) => c.id !== col.id),
                    });
                  } catch (e) {
                    setError(
                      (e as { message?: string }).message ??
                        "Could not delete column.",
                    );
                  }
                }}
              />
            ))}
          </SortableContext>

          {canManage && (
            <AddColumnButton
              onAdd={async (name, category) => {
                const created = await createColumn(board.id, { name, category });
                onBoardChange({
                  ...board,
                  columns: [...board.columns, created],
                });
              }}
            />
          )}
        </div>

        <DragOverlay>
          {activeTask ? <TaskCard task={activeTask} canManage={false} /> : null}
        </DragOverlay>
      </DndContext>
    </div>
  );
}

function ColumnView({
  projectKey,
  projectId,
  column,
  tasks,
  canManage,
  onEdit,
  onDelete,
}: {
  projectKey: string;
  projectId: string;
  column: Column;
  tasks: Task[];
  canManage: boolean;
  onEdit: (
    patch: Partial<Pick<Column, "name" | "category" | "wip_limit">>,
  ) => Promise<void>;
  onDelete: () => Promise<void>;
}) {
  const sortable = useSortable({
    id: column.id,
    data: { kind: "column" },
    disabled: !canManage,
  });
  const style = {
    transform: CSS.Transform.toString(sortable.transform),
    transition: sortable.transition,
    opacity: sortable.isDragging ? 0.6 : 1,
  };
  const [editing, setEditing] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const overLimit =
    column.wip_limit != null && tasks.length > column.wip_limit;

  return (
    <div
      ref={sortable.setNodeRef}
      style={style}
      className="flex h-[70vh] w-72 flex-shrink-0 flex-col rounded-lg border border-white/10 bg-ink-subtle"
    >
      <header className="flex items-center gap-2 border-b border-white/10 px-3 py-2">
        {canManage && (
          <button
            type="button"
            {...sortable.attributes}
            {...sortable.listeners}
            className="cursor-grab text-chrome-dim hover:text-chrome active:cursor-grabbing"
            aria-label="Drag column"
          >
            <GripVertical size={14} />
          </button>
        )}
        <CategoryDot category={column.category} />
        {editing ? (
          <InlineEdit column={column} onSave={async (p) => { await onEdit(p); setEditing(false); }} onCancel={() => setEditing(false)} />
        ) : (
          <button
            type="button"
            onClick={() => canManage && setEditing(true)}
            disabled={!canManage}
            className="mono flex-1 truncate text-left text-sm text-chrome disabled:cursor-default"
          >
            {column.name}
          </button>
        )}
        <span
          className={`mono text-[10px] ${
            overLimit ? "text-red-300" : "text-chrome-dim"
          }`}
          title="cards / wip limit"
        >
          {tasks.length}
          {column.wip_limit != null ? `/${column.wip_limit}` : ""}
        </span>
        {canManage && !editing && (
          <div className="relative">
            <button
              type="button"
              onClick={() => setMenuOpen((v) => !v)}
              className="text-chrome-dim hover:text-chrome"
              aria-label="Column menu"
            >
              <MoreHorizontal size={14} />
            </button>
            {menuOpen && (
              <div className="absolute right-0 top-full z-10 mt-1 w-44 rounded border border-white/10 bg-ink p-1 shadow-xl">
                <button
                  type="button"
                  onClick={() => { setMenuOpen(false); setEditing(true); }}
                  className="mono block w-full rounded px-2 py-1.5 text-left text-xs hover:bg-white/5"
                >
                  rename / edit
                </button>
                <button
                  type="button"
                  onClick={async () => { setMenuOpen(false); await onDelete(); }}
                  className="mono flex w-full items-center gap-2 rounded px-2 py-1.5 text-left text-xs text-red-200 hover:bg-red-500/10"
                >
                  <Trash2 size={11} /> delete
                </button>
              </div>
            )}
          </div>
        )}
      </header>

      <ColumnBody column={column} tasks={tasks} canManage={canManage} />

      {canManage && (
        <AddCardButton projectKey={projectKey} projectId={projectId} columnId={column.id} />
      )}
    </div>
  );
}

function ColumnBody({
  column,
  tasks,
  canManage,
}: {
  column: Column;
  tasks: Task[];
  canManage: boolean;
}) {
  // The body itself is a separate drop zone (id distinct from the column
  // header's sortable id) so empty columns can still accept drops. Detected
  // by suffix in onDragEnd.
  const drop = useDroppable({
    id: `${column.id}:body`,
    data: { kind: "column-body", column_id: column.id },
  });
  return (
    <div
      ref={drop.setNodeRef}
      className={`flex-1 space-y-2 overflow-y-auto p-2 transition ${
        drop.isOver ? "bg-white/5" : ""
      }`}
    >
      <SortableContext
        items={tasks.map((t) => t.id)}
        strategy={verticalListSortingStrategy}
        disabled={!canManage}
      >
        {tasks.map((t) => (
          <TaskCard key={t.id} task={t} canManage={canManage} />
        ))}
      </SortableContext>
      {tasks.length === 0 && (
        <div className="mono pt-2 text-center text-[10px] text-chrome-dim">
          empty — drop a card here
        </div>
      )}
    </div>
  );
}

function AddCardButton({
  projectKey,
  projectId,
  columnId,
}: {
  projectKey: string;
  projectId: string;
  columnId: string;
}) {
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const create = useCreateTask(projectKey, projectId);
  if (!open) {
    return (
      <button
        type="button"
        data-add-card-button
        onClick={() => setOpen(true)}
        className="mono border-t border-white/10 px-3 py-2 text-left text-xs text-chrome-dim hover:bg-white/5 hover:text-chrome"
      >
        <Plus size={12} className="-mt-0.5 mr-1 inline" /> add card
      </button>
    );
  }
  return (
    <form
      onSubmit={async (e) => {
        e.preventDefault();
        if (!title.trim()) return;
        await create.mutateAsync({ title, column_id: columnId });
        setTitle("");
      }}
      className="space-y-1 border-t border-white/10 p-2"
    >
      <input
        autoFocus
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        placeholder="card title"
        className="w-full rounded border border-white/10 bg-ink px-2 py-1 text-sm text-chrome focus:border-accent focus:outline-none"
      />
      <div className="flex items-center justify-between">
        <button
          type="button"
          onClick={() => { setOpen(false); setTitle(""); }}
          className="mono text-[10px] text-chrome-dim hover:text-chrome"
        >
          :q cancel
        </button>
        <button
          type="submit"
          disabled={create.isPending}
          className="mono rounded bg-accent px-2 py-1 text-[10px] text-accent-fg disabled:opacity-50"
        >
          {create.isPending ? "…" : "add"}
        </button>
      </div>
    </form>
  );
}

function InlineEdit({
  column,
  onSave,
  onCancel,
}: {
  column: Column;
  onSave: (
    patch: Partial<Pick<Column, "name" | "category" | "wip_limit">>,
  ) => Promise<void>;
  onCancel: () => void;
}) {
  const [name, setName] = useState(column.name);
  const [category, setCategory] = useState(column.category);
  const [wip, setWip] = useState<string>(column.wip_limit?.toString() ?? "");
  return (
    <form
      className="flex flex-1 items-center gap-1"
      onSubmit={async (e) => {
        e.preventDefault();
        const patch: Partial<Pick<Column, "name" | "category" | "wip_limit">> = {};
        if (name !== column.name) patch.name = name;
        if (category !== column.category) patch.category = category;
        if (wip && Number(wip) !== column.wip_limit) patch.wip_limit = Number(wip);
        await onSave(patch);
      }}
    >
      <input
        autoFocus
        value={name}
        onChange={(e) => setName(e.target.value)}
        className="mono w-full rounded border border-white/10 bg-ink px-1.5 py-0.5 text-xs text-chrome focus:border-accent focus:outline-none"
      />
      <select
        value={category}
        onChange={(e) => setCategory(e.target.value as Column["category"])}
        className="mono rounded border border-white/10 bg-ink px-1 py-0.5 text-xs text-chrome"
        aria-label="category"
      >
        <option value="todo">todo</option>
        <option value="in_progress">in_progress</option>
        <option value="review">review</option>
        <option value="done">done</option>
      </select>
      <input
        value={wip}
        onChange={(e) => setWip(e.target.value.replace(/[^0-9]/g, ""))}
        placeholder="wip"
        className="mono w-12 rounded border border-white/10 bg-ink px-1 py-0.5 text-xs text-chrome"
        aria-label="WIP limit"
      />
      <button type="submit" className="text-accent hover:opacity-80" aria-label="Save">
        <Check size={14} />
      </button>
      <button type="button" onClick={onCancel} className="text-chrome-dim hover:text-chrome" aria-label="Cancel">
        <X size={14} />
      </button>
    </form>
  );
}

function AddColumnButton({
  onAdd,
}: {
  onAdd: (name: string, category: Column["category"]) => Promise<void>;
}) {
  const [open, setOpen] = useState(false);
  const [name, setName] = useState("");
  const [category, setCategory] = useState<Column["category"]>("todo");
  const [busy, setBusy] = useState(false);
  if (!open) {
    return (
      <button
        type="button"
        onClick={() => setOpen(true)}
        className="mono flex h-12 w-72 flex-shrink-0 items-center justify-center gap-2 rounded-lg border border-dashed border-white/10 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
      >
        <Plus size={14} /> add column
      </button>
    );
  }
  return (
    <form
      onSubmit={async (e) => {
        e.preventDefault();
        setBusy(true);
        try {
          await onAdd(name, category);
          setOpen(false);
          setName("");
        } finally {
          setBusy(false);
        }
      }}
      className="flex h-12 w-72 flex-shrink-0 items-center gap-1 rounded-lg border border-white/10 bg-ink-subtle p-2"
    >
      <input
        autoFocus
        value={name}
        onChange={(e) => setName(e.target.value)}
        placeholder="column name"
        className="mono flex-1 rounded border border-white/10 bg-ink px-2 py-1 text-xs text-chrome focus:border-accent focus:outline-none"
      />
      <select
        value={category}
        onChange={(e) => setCategory(e.target.value as Column["category"])}
        className="mono rounded border border-white/10 bg-ink px-1 py-1 text-xs text-chrome"
        aria-label="category"
      >
        <option value="todo">todo</option>
        <option value="in_progress">in_progress</option>
        <option value="review">review</option>
        <option value="done">done</option>
      </select>
      <button
        type="submit"
        disabled={busy || !name}
        className="mono rounded bg-accent px-2 py-1 text-xs text-accent-fg disabled:opacity-50"
      >
        add
      </button>
      <button
        type="button"
        onClick={() => setOpen(false)}
        className="text-chrome-dim hover:text-chrome"
        aria-label="Cancel"
      >
        <X size={14} />
      </button>
    </form>
  );
}

function CategoryDot({ category }: { category: Column["category"] }) {
  const color: Record<Column["category"], string> = {
    todo: "#94a3b8",
    in_progress: "#22d3ee",
    review: "#f59e0b",
    done: "#10b981",
  };
  return (
    <span
      aria-hidden
      className="inline-block h-1.5 w-1.5 rounded-full"
      style={{ background: color[category] }}
    />
  );
}
