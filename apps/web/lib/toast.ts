// Minimal transient-toast bus. One module-level list of subscribers, no
// context, no portal gymnastics — the <Toaster /> in Providers renders
// whatever gets pushed here. Built for the "deleted — undo" pattern, generic
// enough for any short-lived notice with at most one action.

export type Toast = {
  id: number;
  message: string;
  /** Optional action button ("undo"). The toast closes after it runs. */
  actionLabel?: string;
  onAction?: () => void | Promise<void>;
  /** Auto-dismiss after this many ms. */
  ttlMs: number;
};

type Listener = (toasts: Toast[]) => void;

let seq = 0;
let toasts: Toast[] = [];
const listeners = new Set<Listener>();

function emit() {
  for (const fn of listeners) fn(toasts);
}

export function subscribeToasts(fn: Listener): () => void {
  listeners.add(fn);
  fn(toasts);
  return () => {
    listeners.delete(fn);
  };
}

export function dismissToast(id: number) {
  toasts = toasts.filter((t) => t.id !== id);
  emit();
}

export function showToast(
  message: string,
  opts: { actionLabel?: string; onAction?: () => void | Promise<void>; ttlMs?: number } = {},
): number {
  const id = ++seq;
  const toast: Toast = {
    id,
    message,
    actionLabel: opts.actionLabel,
    onAction: opts.onAction,
    ttlMs: opts.ttlMs ?? 6000,
  };
  toasts = [...toasts, toast];
  emit();
  setTimeout(() => dismissToast(id), toast.ttlMs);
  return id;
}
