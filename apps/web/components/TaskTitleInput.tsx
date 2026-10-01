"use client";

// The "new task" field, commit-message style. Looks and behaves like a one-line
// input — Enter files the task, Esc gives up — but it's a textarea underneath,
// because an <input> silently flattens a pasted multi-line message into one
// long title. Paste (or Shift+Enter your way through)
//
//     deploy the application on the server
//
//     The application is ready for deployment…
//
// and the first line becomes the title, the rest the description (QA report 6).
// Callers split with `splitCommitMessage` on submit.

import { forwardRef, useEffect, useImperativeHandle, useLayoutEffect, useRef } from "react";
import { hasBody, splitCommitMessage } from "@/lib/commitMessage";

type Props = {
  value: string;
  onChange: (v: string) => void;
  /** Enter without Shift. */
  onSubmit: () => void;
  onEscape?: () => void;
  /** Runs first; call `preventDefault()` to claim a key (e.g. arrow keys). */
  onKeyDown?: (e: React.KeyboardEvent<HTMLTextAreaElement>) => void;
  placeholder?: string;
  autoFocus?: boolean;
  className?: string;
  "aria-label"?: string;
  "aria-invalid"?: boolean;
};

export const TaskTitleInput = forwardRef<HTMLTextAreaElement, Props>(function TaskTitleInput(
  { value, onChange, onSubmit, onEscape, onKeyDown, className = "", ...rest },
  ref,
) {
  const inner = useRef<HTMLTextAreaElement>(null);
  useImperativeHandle(ref, () => inner.current!);

  // Uncontrolled on purpose. React mirrors a *controlled* textarea's value
  // into its default value — i.e. its text content — so the half-typed title
  // became page text: anything looking for the new card by its title (a
  // person scanning the board, a test, a screen reader's find) hit the input
  // first. We push the parent's value into the DOM ourselves instead, only
  // when it differs (a reset after submit, a restore after an error).
  useLayoutEffect(() => {
    const el = inner.current;
    if (el && el.value !== value) el.value = value;
  }, [value]);

  // Grow with the content, one line by default, capped so a pasted essay
  // scrolls instead of shoving the board around.
  useEffect(() => {
    const el = inner.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
  }, [value]);

  const split = hasBody(value) ? splitCommitMessage(value) : null;

  return (
    <>
      <textarea
        ref={inner}
        rows={1}
        onChange={(e) => onChange(e.target.value)}
        onKeyDown={(e) => {
          onKeyDown?.(e);
          if (e.defaultPrevented) return;
          // Mid-composition Enter (IME) picks a candidate; it isn't a submit.
          if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
            e.preventDefault();
            onSubmit();
          } else if (e.key === "Escape" && onEscape) {
            e.preventDefault();
            onEscape();
          }
        }}
        className={`resize-none overflow-y-auto ${className}`}
        {...rest}
      />
      {split && (
        <div className="mono text-[10px] leading-snug text-chrome-dim" data-commit-split>
          title: <span className="text-chrome">{split.title}</span>
          <br />+ description, {split.description.split("\n").length}{" "}
          {split.description.split("\n").length === 1 ? "line" : "lines"}
        </div>
      )}
    </>
  );
});
