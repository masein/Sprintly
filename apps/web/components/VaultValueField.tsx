"use client";

// The secret-value field, shared by the vault's create and edit forms.
//
// For file-shaped kinds (env_file, ssh_key) you can drop a file onto it — the
// text is read in the browser and becomes the value, so the file itself never
// travels anywhere before the normal encrypt-on-submit path. Typing still
// works; the drop zone is a shortcut, not a replacement.

import { useRef, useState } from "react";
import { Upload } from "lucide-react";

/** Refuse absurd pastes early — the API caps the value at 64 KiB. */
const MAX_BYTES = 65_536;

export function VaultValueField({
  kind,
  value,
  onChange,
  label = "the actual secret value",
  required,
}: {
  kind: string;
  value: string;
  onChange: (v: string) => void;
  label?: string;
  required?: boolean;
}) {
  const fileish = kind === "env_file" || kind === "ssh_key";
  const [dragging, setDragging] = useState(false);
  const [dropped, setDropped] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const picker = useRef<HTMLInputElement>(null);

  async function take(file: File) {
    setError(null);
    if (file.size > MAX_BYTES) {
      setError(`${file.name} is larger than 64 KiB — paste the part you need.`);
      return;
    }
    const text = await file.text();
    onChange(text);
    setDropped(`${file.name} · ${text.split("\n").length} lines`);
  }

  return (
    <div className="space-y-1">
      <textarea
        value={value}
        onChange={(e) => {
          onChange(e.target.value);
          setDropped(null);
        }}
        required={required}
        rows={fileish ? 6 : 2}
        aria-label={label}
        placeholder={label}
        onDragOver={(e) => {
          if (!fileish) return;
          e.preventDefault();
          setDragging(true);
        }}
        onDragLeave={() => setDragging(false)}
        onDrop={(e) => {
          if (!fileish) return;
          e.preventDefault();
          setDragging(false);
          const file = e.dataTransfer.files?.[0];
          if (file) void take(file);
        }}
        className={`mono block w-full rounded border bg-ink px-2 py-1 text-sm text-chrome focus:border-accent focus:outline-none ${
          dragging ? "border-accent bg-accent/5" : "border-white/10"
        }`}
      />
      {fileish && (
        <div className="mono flex flex-wrap items-center gap-2 text-[10px] text-chrome-dim">
          <span>{dragging ? "drop it —" : "drag a file in, or"}</span>
          <button
            type="button"
            onClick={() => picker.current?.click()}
            className="inline-flex items-center gap-1 rounded border border-white/10 px-1.5 py-0.5 text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            <Upload size={9} /> choose a file
          </button>
          <input
            ref={picker}
            type="file"
            aria-label="secret file"
            className="hidden"
            onChange={(e) => {
              const file = e.target.files?.[0];
              if (file) void take(file);
              e.target.value = "";
            }}
          />
          {dropped && <span className="text-accent">loaded {dropped}</span>}
          <span>· read in your browser, encrypted like any other secret</span>
        </div>
      )}
      {error && <div className="mono text-[11px] text-red-200">{error}</div>}
    </div>
  );
}
