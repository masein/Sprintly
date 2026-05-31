"use client";

// One row in the vault list. Owns the reveal lifecycle for its own item:
// click-to-reveal renders plaintext in local state for 10s with a countdown
// then masks it. Copy uses the Clipboard API + a 30s auto-clear timer.
//
// Plaintext is wiped on unmount (the local state goes out of scope) and on
// route change (React unmounts the page-level tree). It never enters
// TanStack cache or any global store.

import { useEffect, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  Eye, EyeOff, Copy, Trash2, RotateCw, History, Key, FileText, Server, KeyRound, Terminal,
} from "lucide-react";
import {
  deleteVaultItem,
  markCopied,
  revealVaultItem,
  type VaultItem,
} from "@/lib/vault";
import type { ApiError } from "@/lib/api";

const REVEAL_TTL_MS = 10_000;
const CLIPBOARD_TTL_MS = 30_000;

const KIND_ICON = {
  password: Key,
  api_key: KeyRound,
  ssh_key: Server,
  note: FileText,
  env_file: Terminal,
} as const;

export function VaultItemRow({
  item,
  canEdit,
  onShowAudit,
  onShowAccess,
  projectKey,
}: {
  item: VaultItem;
  canEdit: boolean;
  onShowAudit: (id: string) => void;
  onShowAccess: (id: string) => void;
  projectKey: string;
}) {
  const qc = useQueryClient();

  // Plaintext state — local only. Wiped on unmount.
  const [revealed, setRevealed] = useState<string | null>(null);
  const [remaining, setRemaining] = useState(0);
  const revealTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const tickTimer = useRef<ReturnType<typeof setInterval> | null>(null);
  const clipTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Always wipe on unmount.
  useEffect(() => {
    return () => {
      setRevealed(null);
      if (revealTimer.current) clearTimeout(revealTimer.current);
      if (tickTimer.current) clearInterval(tickTimer.current);
      if (clipTimer.current) clearTimeout(clipTimer.current);
    };
  }, []);

  const reveal = useMutation({
    mutationFn: () => revealVaultItem(item.id),
    onSuccess: (r) => {
      setRevealed(r.value);
      setRemaining(REVEAL_TTL_MS / 1000);
      if (tickTimer.current) clearInterval(tickTimer.current);
      tickTimer.current = setInterval(() => {
        setRemaining((s) => (s > 0 ? s - 1 : 0));
      }, 1000);
      if (revealTimer.current) clearTimeout(revealTimer.current);
      revealTimer.current = setTimeout(() => {
        setRevealed(null);
        if (tickTimer.current) clearInterval(tickTimer.current);
      }, REVEAL_TTL_MS);
    },
    onError: (e) => {
      const err = e as unknown as ApiError;
      alert(err.status === 429 ? "Reveal rate limit hit. Try again in an hour." : err.message);
    },
  });

  async function copyToClipboard() {
    // Reveal first if we don't already have it in memory.
    let plaintext = revealed;
    if (!plaintext) {
      try {
        const r = await revealVaultItem(item.id);
        plaintext = r.value;
      } catch (e) {
        const err = e as unknown as ApiError;
        alert(err.status === 429 ? "Reveal rate limit hit." : err.message);
        return;
      }
    }
    try {
      await navigator.clipboard.writeText(plaintext);
      await markCopied(item.id).catch(() => {});
      // Schedule auto-wipe.
      if (clipTimer.current) clearTimeout(clipTimer.current);
      clipTimer.current = setTimeout(async () => {
        // Best-effort overwrite. Some browsers reject writes without a user
        // gesture; we then write empty which still clears in the common case.
        try {
          await navigator.clipboard.writeText("");
        } catch {
          /* nothing else to do */
        }
      }, CLIPBOARD_TTL_MS);
    } catch {
      alert("Clipboard write failed — your browser blocked it.");
    }
  }

  const del = useMutation({
    mutationFn: () => deleteVaultItem(item.id),
    onSuccess: () => qc.invalidateQueries({ queryKey: ["vault", projectKey] }),
  });

  const Icon = KIND_ICON[item.kind] ?? Key;

  return (
    <li className="rounded-lg border border-white/10 bg-ink-subtle p-3">
      <div className="flex items-center gap-3">
        <Icon size={14} className="flex-shrink-0 text-chrome-dim" />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <span className="text-sm text-chrome">{item.name}</span>
            <span className="mono rounded border border-white/10 px-1 py-0.5 text-[9px] uppercase tracking-widest text-chrome-dim">
              {item.kind.replace("_", " ")}
            </span>
          </div>
          {item.description && (
            <div className="mono mt-0.5 text-[11px] text-chrome-dim">
              {item.description}
            </div>
          )}
        </div>
        <button
          type="button"
          onClick={() => onShowAudit(item.id)}
          className="text-chrome-dim hover:text-chrome"
          title="audit log"
          aria-label="audit log"
        >
          <History size={13} />
        </button>
        {canEdit && (
          <button
            type="button"
            onClick={() => onShowAccess(item.id)}
            className="text-chrome-dim hover:text-chrome"
            title="manage access"
            aria-label="manage access"
          >
            <Eye size={13} />
          </button>
        )}
        {canEdit && (
          <button
            type="button"
            onClick={() => {
              if (confirm("Delete this vault item? Soft delete; admins can recover.")) {
                del.mutate();
              }
            }}
            className="text-chrome-dim hover:text-red-300"
            aria-label="delete"
          >
            <Trash2 size={13} />
          </button>
        )}
      </div>

      <div className="mt-2 flex items-center gap-2 border-t border-white/5 pt-2">
        {revealed ? (
          <>
            <span className="mono flex-1 overflow-x-auto whitespace-pre-wrap break-all rounded bg-ink px-2 py-1 text-xs text-chrome">
              {revealed}
            </span>
            <span className="mono text-[10px] text-chrome-dim" aria-live="polite">
              hides in {remaining}s
            </span>
            <button
              type="button"
              onClick={() => {
                setRevealed(null);
                if (revealTimer.current) clearTimeout(revealTimer.current);
                if (tickTimer.current) clearInterval(tickTimer.current);
              }}
              className="text-chrome-dim hover:text-chrome"
              aria-label="hide"
            >
              <EyeOff size={13} />
            </button>
          </>
        ) : (
          <>
            <span className="mono flex-1 rounded bg-ink px-2 py-1 text-xs tracking-widest text-chrome-dim">
              ••••••••••••••••
            </span>
            <button
              type="button"
              onClick={() => reveal.mutate()}
              disabled={reveal.isPending}
              className="mono inline-flex items-center gap-1 rounded border border-white/10 px-2 py-1 text-[11px] text-chrome-dim hover:border-white/20 hover:text-chrome disabled:opacity-50"
            >
              <Eye size={11} /> reveal
            </button>
          </>
        )}
        <button
          type="button"
          onClick={copyToClipboard}
          className="mono inline-flex items-center gap-1 rounded border border-white/10 px-2 py-1 text-[11px] text-chrome-dim hover:border-white/20 hover:text-chrome"
          title="copy (clears in 30s)"
        >
          <Copy size={11} /> copy
        </button>
      </div>

      <div className="mono mt-2 text-[10px] text-chrome-dim">
        v{item.key_version} · rotated {item.last_rotated_at.slice(0, 10)}
        {canEdit && (
          <span className="ml-2 inline-flex items-center gap-1">
            <RotateCw size={9} /> patch with new value to rotate
          </span>
        )}
      </div>
    </li>
  );
}
