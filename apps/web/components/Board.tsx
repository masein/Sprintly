"use client";

// The Kanban board.
//
//   • Columns with task cards; drag-reorder columns; drag tasks within/across
//     columns (optimistic, server-authoritative, reconciled via WS/refetch).
//   • Inline add-card / add-column; per-column edit/delete.
//   • Filter chips + saved views + swimlanes (F8): pick a saved view to restore
//     its filter + grouping, or group the board into lanes by assignee / label
//     / priority. In a grouped view, cards still drag between columns within
//     their lane; column management stays in the ungrouped view.

import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
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
  listMembers,
  reorderColumns,
  type Board as BoardModel,
  type Column,
} from "@/lib/projects";
import { useCreateTask, useMoveTask, useTasks, type Task } from "@/lib/tasks";
import { listSprints, type Sprint } from "@/lib/sprints";
import { type BoardView, type GroupBy } from "@/lib/boardViews";
import { TaskCard } from "./TaskCard";
import { BoardFilters, toFilterDSL, type Chip } from "./BoardFilters";
import { BoardViewBar } from "./BoardViewBar";
import { ListSearch, matchesTask } from "./ListSearch";

type Lane = { key: string; label: string; tasks: Task[] };

// Fields a new card inherits from the lane it's added into (QA F13), so a card
// quick-added in the "@bob" / "p1" / "backend" lane actually lands in that lane.
// `sprint_id` comes from the board's scope (not a lane) so a card added while
// scoped to a sprint joins that sprint and stays visible in the filtered view.
type CardDefaults = {
  assignee_id?: string;
  priority?: Task["priority"];
  labels?: string[];
  sprint_id?: string;
};

const PRIORITY_ORDER = ["p0", "p1", "p2", "p3"];
const UNGROUPED = "\0"; // sorts last for the "unassigned"/"no label" lane

/// Partition tasks into swimlanes for the chosen grouping. Only non-empty
/// lanes render; the catch-all lane (unassigned / no label) sorts last.
function computeLanes(
  tasks: Task[],
  groupBy: GroupBy,
  memberName: (id: string) => string,
  sprints: Sprint[],
  activeSprintId: string | null,
): Lane[] {
  if (groupBy === "none") return [{ key: "all", label: "", tasks }];
  if (groupBy === "sprint") return computeSprintLanes(tasks, sprints, activeSprintId);

  const buckets = new Map<string, Task[]>();
  const push = (k: string, t: Task) => {
    const list = buckets.get(k);
    if (list) list.push(t);
    else buckets.set(k, [t]);
  };

  for (const t of tasks) {
    if (groupBy === "priority") push(t.priority, t);
    else if (groupBy === "assignee") push(t.assignee_id ?? UNGROUPED, t);
    else if (groupBy === "label") {
      if (t.labels.length === 0) push(UNGROUPED, t);
      else for (const l of t.labels) push(l, t);
    }
  }

  const labelFor = (key: string): string => {
    if (key === UNGROUPED) return groupBy === "assignee" ? "unassigned" : "no label";
    if (groupBy === "assignee") return memberName(key);
    return key;
  };
  const rank = (key: string): [number, string] => {
    if (key === UNGROUPED) return [2, ""];
    if (groupBy === "priority") return [0, String(PRIORITY_ORDER.indexOf(key)).padStart(2, "0")];
    return [1, labelFor(key).toLowerCase()];
  };

  return [...buckets.entries()]
    .map(([key, ts]) => ({ key, label: labelFor(key), tasks: ts }))
    .sort((a, b) => {
      const [ra, sa] = rank(a.key);
      const [rb, sb] = rank(b.key);
      return ra - rb || sa.localeCompare(sb);
    });
}

/// Sprint swimlanes: the active sprint on top (the point is to separate
/// committed work from the rest), then any other sprints with cards in view
/// (usually completed — a look-back), most recent start first, and the
/// backlog / no-sprint lane last. Only non-empty lanes render.
function computeSprintLanes(
  tasks: Task[],
  sprints: Sprint[],
  activeSprintId: string | null,
): Lane[] {
  const byId = new Map(sprints.map((s) => [s.id, s]));
  const buckets = new Map<string, Task[]>();
  for (const t of tasks) {
    const k = t.sprint_id ?? UNGROUPED;
    const list = buckets.get(k);
    if (list) list.push(t);
    else buckets.set(k, [t]);
  }

  const lanes: Lane[] = [];

  if (activeSprintId && buckets.has(activeSprintId)) {
    const s = byId.get(activeSprintId);
    lanes.push({
      key: activeSprintId,
      label: s ? `${s.name} · active` : "active sprint",
      tasks: buckets.get(activeSprintId)!,
    });
    buckets.delete(activeSprintId);
  }

  [...buckets.keys()]
    .filter((k) => k !== UNGROUPED)
    .sort((a, b) => (byId.get(b)?.starts_at ?? "").localeCompare(byId.get(a)?.starts_at ?? ""))
    .forEach((k) => {
      const s = byId.get(k);
      lanes.push({ key: k, label: s ? s.name : "unknown sprint", tasks: buckets.get(k)! });
    });

  const backlog = buckets.get(UNGROUPED);
  if (backlog) lanes.push({ key: UNGROUPED, label: "backlog · no sprint", tasks: backlog });

  return lanes;
}

/// What a card added into a given lane should inherit so it stays in that lane.
/// The catch-all lane (unassigned / no label / backlog) adds nothing.
function laneCardDefaults(groupBy: GroupBy, laneKey: string): CardDefaults {
  if (laneKey === UNGROUPED) return {};
  if (groupBy === "assignee") return { assignee_id: laneKey };
  if (groupBy === "priority") return { priority: laneKey as Task["priority"] };
  if (groupBy === "label") return { labels: [laneKey] };
  if (groupBy === "sprint") return { sprint_id: laneKey };
  return {};
}

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
  const [groupBy, setGroupBy] = useState<GroupBy>("none");
  const [activeViewId, setActiveViewId] = useState<string | null>(null);

  // Board scope (F10): "active" | "all" | a specific sprint id. Null until we've
  // loaded the sprint list and can resolve a sensible default.
  const [scope, setScope] = useState<string | null>(null);

  const sprintsQ = useQuery({
    queryKey: ["sprints", projectKey],
    queryFn: () => listSprints(projectKey),
    retry: false,
  });
  const sprints = useMemo(() => sprintsQ.data ?? [], [sprintsQ.data]);
  const activeSprint = sprints.find((s) => s.state === "active") ?? null;

  // Resolve the initial scope once the sprints have loaded. A running sprint
  // wins on every fresh open — that's how a sprint board is expected to behave
  // (you land on what you committed to), matching Jira. The choice is NOT
  // persisted across loads: switching to "all tasks" (or pinning a past sprint)
  // is a session-only move, and a reload snaps back to the active sprint. With
  // no sprint running the board opens on "all tasks".
  useEffect(() => {
    if (scope !== null || !sprintsQ.isSuccess) return;
    setScope(activeSprint ? "active" : "all");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sprintsQ.isSuccess]);

  function changeScope(next: string) {
    setScope(next);
  }

  // The concrete sprint a new card should join so it doesn't vanish from the
  // current filtered view: the active sprint's id when scoped to "active", the
  // pinned sprint's id when a specific sprint is selected, or undefined for
  // "all tasks" (a sprint-less backlog card, today's behavior).
  const scopedSprintId =
    scope === null || scope === "all"
      ? undefined
      : scope === "active"
        ? (activeSprint?.id ?? undefined)
        : scope;

  // Free-text search over what's already loaded: key, title, labels. The chips
  // are a server-side filter DSL; this is the "where is that card" box QA
  // asked for, and it stays instant by never leaving the browser.
  const [search, setSearch] = useState("");

  const filter = chips.length > 0 ? toFilterDSL(chips) : undefined;
  // Don't fetch until the scope is resolved — avoids a flash of the wrong scope.
  const { data: allTasks = [] } = useTasks(
    projectKey,
    projectId,
    filter,
    scope ?? "all",
    scope !== null,
  );
  const move = useMoveTask(projectId);
  const tasks = useMemo(
    () => (search.trim() ? allTasks.filter((t) => matchesTask(search, t)) : allTasks),
    [allTasks, search],
  );

  // Member handles for assignee swimlane labels (only fetched when needed).
  const membersQ = useQuery({
    queryKey: ["project-members", projectKey],
    queryFn: () => listMembers(projectKey),
    enabled: groupBy === "assignee",
    retry: false,
  });
  const memberName = (id: string) => {
    const m = membersQ.data?.find((x) => x.user_id === id);
    return m ? `@${m.handle}` : "someone";
  };

  const lanes = useMemo(
    () => computeLanes(tasks, groupBy, memberName, sprints, activeSprint?.id ?? null),
    // memberName closes over membersQ.data; depend on it explicitly.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [tasks, groupBy, membersQ.data, sprints, activeSprint?.id],
  );

  function applyView(v: BoardView) {
    setChips(v.filter ?? []);
    setGroupBy(v.group_by);
    setActiveViewId(v.id);
  }

  return (
    <div>
      <BoardViewBar
        projectKey={projectKey}
        chips={chips}
        groupBy={groupBy}
        activeViewId={activeViewId}
        onApplyView={applyView}
        onGroupByChange={(g) => { setGroupBy(g); setActiveViewId(null); }}
        sprints={sprints}
        activeSprintId={activeSprint?.id ?? null}
        scope={scope ?? "all"}
        onScopeChange={changeScope}
      />
      <BoardFilters
        chips={chips}
        onChange={(next) => { setChips(next); setActiveViewId(null); }}
      />
      <div className="mb-3 flex items-center gap-2">
        <ListSearch
          value={search}
          onChange={setSearch}
          label="search board tasks"
          placeholder="search cards by key, title, or label…"
          className="w-full sm:w-80"
        />
        {search.trim() && (
          <span className="mono shrink-0 text-[11px] text-chrome-dim">
            {tasks.length} of {allTasks.length}
          </span>
        )}
      </div>
      {error && (
        <div className="mono mb-3 rounded border border-red-500/30 bg-red-500/10 p-2 text-xs text-red-200">
          {error}
        </div>
      )}

      {groupBy === "none" ? (
        <BoardSurface
          projectKey={projectKey}
          projectId={projectId}
          board={board}
          tasks={tasks}
          canMoveCards={canManage}
          manageColumns={canManage}
          canAddCards={canManage}
          cardDefaults={{ sprint_id: scopedSprintId }}
          move={move}
          onBoardChange={onBoardChange}
          onError={setError}
        />
      ) : lanes.length === 0 ? (
        <div className="mono rounded border border-dashed border-white/10 p-8 text-center text-sm text-chrome-dim">
          nothing to lane up — no cards match this view.
        </div>
      ) : (
        <div className="space-y-5">
          {lanes.map((lane) => (
            <section key={lane.key}>
              <div
                data-testid="lane-header"
                className="mono mb-1 flex items-center gap-2 text-xs uppercase tracking-widest text-chrome-dim"
              >
                {lane.label}
                <span className="text-chrome-dim/60">· {lane.tasks.length}</span>
              </div>
              <BoardSurface
                projectKey={projectKey}
                projectId={projectId}
                board={board}
                tasks={lane.tasks}
                canMoveCards={canManage}
                manageColumns={false}
                canAddCards={canManage}
                cardDefaults={
                  // The lane's own defaults (incl. its sprint when grouped by
                  // sprint); the board scope's sprint overrides only when the
                  // scope actually pins one, else the lane's sprint stands.
                  scopedSprintId
                    ? { ...laneCardDefaults(groupBy, lane.key), sprint_id: scopedSprintId }
                    : laneCardDefaults(groupBy, lane.key)
                }
                move={move}
                onBoardChange={onBoardChange}
                onError={setError}
                laneId={lane.key}
              />
            </section>
          ))}
        </div>
      )}
    </div>
  );
}

// ─── BoardSurface: the DnD column board for one set of tasks ─────────────────

function BoardSurface({
  projectKey,
  projectId,
  board,
  tasks,
  canMoveCards,
  manageColumns,
  canAddCards,
  cardDefaults,
  move,
  onBoardChange,
  onError,
  laneId,
}: {
  projectKey: string;
  projectId: string;
  board: BoardModel;
  tasks: Task[];
  canMoveCards: boolean;
  manageColumns: boolean;
  canAddCards: boolean;
  cardDefaults?: CardDefaults;
  move: ReturnType<typeof useMoveTask>;
  onBoardChange: (next: BoardModel) => void;
  onError: (msg: string | null) => void;
  laneId?: string;
}) {
  const tasksByColumn = useMemo(() => {
    const m = new Map<string, Task[]>();
    for (const c of board.columns) m.set(c.id, []);
    for (const t of tasks) m.get(t.column_id)?.push(t);
    for (const list of m.values()) list.sort((a, b) => a.order_in_column - b.order_in_column);
    return m;
  }, [board.columns, tasks]);

  const [activeTask, setActiveTask] = useState<Task | null>(null);

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 6 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  function onDragStart(e: DragStartEvent) {
    const t = tasks.find((x) => x.id === String(e.active.id));
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
        onError("Column reorder rejected.");
      }
      return;
    }

    const movingTask = tasks.find((t) => t.id === active.id);
    if (!movingTask) return;

    const overId = String(over.id);
    const overTask = tasks.find((t) => t.id === overId);
    // Column-body drop zones are `${columnId}:body` on the plain board and
    // `${columnId}:body:${laneKey}` inside a swimlane (ids must be unique
    // across lanes). Match both — matching only the bare suffix silently ate
    // every column-body drop in grouped views, so cards could only move
    // between columns by landing exactly on another card.
    const bodyColumnId = overId.includes(":body") ? (overId.split(":body")[0] ?? null) : null;
    const overColumn = bodyColumnId ? board.columns.find((c) => c.id === bodyColumnId) : null;

    let payload: Parameters<typeof move.mutate>[0] | null = null;
    if (overTask) {
      if (overTask.id === movingTask.id) return;
      payload = { taskKey: movingTask.key, column_id: overTask.column_id, before_task_id: overTask.id };
    } else if (overColumn) {
      payload = { taskKey: movingTask.key, column_id: overColumn.id };
    }
    if (payload) move.mutate(payload);
  }

  // dnd-kit ids must be unique across the page; lanes share column ids, so
  // suffix the column-body drop zones per lane.
  const bodySuffix = laneId ? `:${laneId}` : "";

  return (
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
          disabled={!manageColumns}
        >
          {board.columns.map((col) => (
            <ColumnView
              key={col.id}
              projectKey={projectKey}
              projectId={projectId}
              column={col}
              tasks={tasksByColumn.get(col.id) ?? []}
              manageColumns={manageColumns}
              canMoveCards={canMoveCards}
              canAddCards={canAddCards}
              cardDefaults={cardDefaults}
              bodySuffix={bodySuffix}
              onEdit={async (patch) => {
                const updated = await editColumn(col.id, patch);
                onBoardChange({
                  ...board,
                  columns: board.columns.map((c) => (c.id === col.id ? updated : c)),
                });
              }}
              onDelete={async () => {
                try {
                  await deleteColumn(col.id);
                  onBoardChange({ ...board, columns: board.columns.filter((c) => c.id !== col.id) });
                } catch (e) {
                  onError((e as { message?: string }).message ?? "Could not delete column.");
                }
              }}
            />
          ))}
        </SortableContext>

        {manageColumns && (
          <AddColumnButton
            onAdd={async (name, category) => {
              const created = await createColumn(board.id, { name, category });
              onBoardChange({ ...board, columns: [...board.columns, created] });
            }}
          />
        )}
      </div>

      <DragOverlay>
        {activeTask ? <TaskCard task={activeTask} canManage={false} /> : null}
      </DragOverlay>
    </DndContext>
  );
}

function ColumnView({
  projectKey,
  projectId,
  column,
  tasks,
  manageColumns,
  canMoveCards,
  canAddCards,
  cardDefaults,
  bodySuffix,
  onEdit,
  onDelete,
}: {
  projectKey: string;
  projectId: string;
  column: Column;
  tasks: Task[];
  manageColumns: boolean;
  canMoveCards: boolean;
  canAddCards: boolean;
  cardDefaults?: CardDefaults;
  bodySuffix: string;
  onEdit: (patch: Partial<Pick<Column, "name" | "category" | "wip_limit">>) => Promise<void>;
  onDelete: () => Promise<void>;
}) {
  const sortable = useSortable({ id: column.id, data: { kind: "column" }, disabled: !manageColumns });
  const style = {
    transform: CSS.Transform.toString(sortable.transform),
    transition: sortable.transition,
    opacity: sortable.isDragging ? 0.6 : 1,
  };
  const [editing, setEditing] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const overLimit = column.wip_limit != null && tasks.length > column.wip_limit;

  return (
    <div
      ref={sortable.setNodeRef}
      style={style}
      className="flex max-h-[70vh] w-[82vw] max-w-xs flex-shrink-0 flex-col rounded-lg border border-white/10 bg-ink-subtle sm:w-72"
    >
      <header className="flex items-center gap-2 border-b border-white/10 px-3 py-2">
        {manageColumns && (
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
            onClick={() => manageColumns && setEditing(true)}
            disabled={!manageColumns}
            className="mono flex-1 truncate text-left text-sm text-chrome disabled:cursor-default"
          >
            {column.name}
          </button>
        )}
        <span
          className={`mono text-[10px] ${overLimit ? "text-red-300" : "text-chrome-dim"}`}
          title="cards / wip limit"
        >
          {tasks.length}
          {column.wip_limit != null ? `/${column.wip_limit}` : ""}
        </span>
        {manageColumns && !editing && (
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

      <ColumnBody column={column} tasks={tasks} canMoveCards={canMoveCards} bodySuffix={bodySuffix} />

      {canAddCards && (
        <AddCardButton
          projectKey={projectKey}
          projectId={projectId}
          columnId={column.id}
          defaults={cardDefaults}
        />
      )}
    </div>
  );
}

function ColumnBody({
  column,
  tasks,
  canMoveCards,
  bodySuffix,
}: {
  column: Column;
  tasks: Task[];
  canMoveCards: boolean;
  bodySuffix: string;
}) {
  const drop = useDroppable({
    id: `${column.id}:body${bodySuffix}`,
    data: { kind: "column-body", column_id: column.id },
  });
  return (
    <div
      ref={drop.setNodeRef}
      className={`min-h-[3rem] flex-1 space-y-2 overflow-y-auto p-2 transition ${drop.isOver ? "bg-white/5" : ""}`}
    >
      <SortableContext
        items={tasks.map((t) => t.id)}
        strategy={verticalListSortingStrategy}
        disabled={!canMoveCards}
      >
        {tasks.map((t) => (
          <TaskCard key={t.id} task={t} canManage={canMoveCards} />
        ))}
      </SortableContext>
      {tasks.length === 0 && (
        <div className="mono pt-2 text-center text-[10px] text-chrome-dim">empty</div>
      )}
    </div>
  );
}

function AddCardButton({
  projectKey,
  projectId,
  columnId,
  defaults,
}: {
  projectKey: string;
  projectId: string;
  columnId: string;
  defaults?: CardDefaults;
}) {
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const create = useCreateTask(projectKey, projectId);
  function close() {
    setOpen(false);
    setTitle("");
  }
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
        await create.mutateAsync({ title, column_id: columnId, ...defaults });
        setTitle("");
      }}
      className="space-y-1 border-t border-white/10 p-2"
    >
      <input
        autoFocus
        value={title}
        onChange={(e) => setTitle(e.target.value)}
        onKeyDown={(e) => {
          // Esc dismisses the inline add-card input (QA F9).
          if (e.key === "Escape") {
            e.preventDefault();
            close();
          }
        }}
        placeholder="card title"
        className="w-full rounded border border-white/10 bg-ink px-2 py-1 text-sm text-chrome focus:border-accent focus:outline-none"
      />
      <div className="flex items-center justify-between">
        <button
          type="button"
          onClick={close}
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
  onSave: (patch: Partial<Pick<Column, "name" | "category" | "wip_limit">>) => Promise<void>;
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
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            onCancel();
          }
        }}
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
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            setOpen(false);
          }
        }}
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
      <button type="button" onClick={() => setOpen(false)} className="text-chrome-dim hover:text-chrome" aria-label="Cancel">
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
