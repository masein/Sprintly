"use client";

// Global keyboard hotkeys. Mounted once in Providers.
//
//   Cmd/Ctrl+K        → open command palette
//   /                 → open palette
//   ?                 → open palette in help mode (sends "?" as query)
//   g p / g m / g s   → navigate to projects / my-tasks / settings
//   c                 → focus the leftmost board's "add card" (best-effort
//                       — uses the data-add-card-button hook below)
//
// Ignored when an input/textarea/contenteditable is focused.

import { useEffect, useRef, useState } from "react";
import { useRouter } from "next/navigation";
import { CommandPalette } from "./CommandPalette";

const CHORD_TIMEOUT_MS = 1500;

export function KeyboardHotkeys() {
  const router = useRouter();
  const [open, setOpen] = useState(false);
  const [initialQuery, setInitialQuery] = useState("");
  const lastChordKey = useRef<{ key: string; ts: number } | null>(null);

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      // Cmd/Ctrl+K — always opens, regardless of focus.
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setInitialQuery("");
        setOpen(true);
        return;
      }
      if (isInInput(e)) return;

      // Single-key triggers.
      if (e.key === "/") {
        e.preventDefault();
        setInitialQuery("");
        setOpen(true);
        return;
      }
      if (e.key === "?") {
        e.preventDefault();
        setInitialQuery("?");
        setOpen(true);
        return;
      }
      if (e.key.toLowerCase() === "c") {
        // Click the leftmost add-card button if visible.
        const btn = document.querySelector<HTMLElement>(
          "[data-add-card-button]",
        );
        if (btn) {
          e.preventDefault();
          btn.click();
        }
        return;
      }

      // Chord: "g" → next-key.
      const now = Date.now();
      const last = lastChordKey.current;
      if (last && last.key === "g" && now - last.ts < CHORD_TIMEOUT_MS) {
        const k = e.key.toLowerCase();
        const dest =
          k === "p" ? "/projects" :
          k === "m" ? "/me/tasks" :
          k === "d" ? "/me/day" :
          k === "s" ? "/settings" :
          null;
        if (dest) {
          e.preventDefault();
          lastChordKey.current = null;
          router.push(dest);
          return;
        }
        lastChordKey.current = null;
      }
      if (e.key.toLowerCase() === "g") {
        lastChordKey.current = { key: "g", ts: now };
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [router]);

  return (
    <CommandPalette
      open={open}
      onOpenChange={setOpen}
      initialQuery={initialQuery}
    />
  );
}

function isInInput(e: KeyboardEvent): boolean {
  const t = e.target as HTMLElement | null;
  if (!t) return false;
  if (t.isContentEditable) return true;
  const tag = t.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || tag === "SELECT";
}
