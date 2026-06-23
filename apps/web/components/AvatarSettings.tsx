"use client";

// Avatar editor for /settings: pick a generated style, regenerate the seed,
// or upload an image (downscaled to a small PNG data URL so it stays inline —
// no object storage, works on an air-gapped self-hosted box). "Use generated"
// clears an upload; the avatar always falls back to the deterministic default.

import { useRef, useState } from "react";
import { setMyAvatar, type Me, type ApiError } from "@/lib/auth-bundle";
import { AVATAR_STYLES, asAvatarStyle, type AvatarStyle } from "@/lib/avatar";
import { Avatar } from "./Avatar";

const STYLE_LABEL: Record<AvatarStyle, string> = {
  beaver: "beaver",
  robot: "robot",
  identicon: "identicon",
  glyph: "emoji",
};

// Downscale any picked image to a square PNG data URL (max 96px).
function fileToDataUrl(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onerror = () => reject(new Error("could not read the file"));
    reader.onload = () => {
      const img = new Image();
      img.onerror = () => reject(new Error("that doesn't look like an image"));
      img.onload = () => {
        const size = 96;
        const canvas = document.createElement("canvas");
        canvas.width = size;
        canvas.height = size;
        const ctx = canvas.getContext("2d");
        if (!ctx) return reject(new Error("canvas unavailable"));
        // Cover-crop to a centred square.
        const side = Math.min(img.width, img.height);
        const sx = (img.width - side) / 2;
        const sy = (img.height - side) / 2;
        ctx.drawImage(img, sx, sy, side, side, 0, 0, size, size);
        resolve(canvas.toDataURL("image/png"));
      };
      img.src = reader.result as string;
    };
    reader.readAsDataURL(file);
  });
}

function randomSeed(): string {
  return Math.random().toString(36).slice(2, 10);
}

export function AvatarSettings({
  user,
  onUpdated,
}: {
  user: Me;
  onUpdated: (m: Me) => void;
}) {
  const [style, setStyle] = useState<AvatarStyle>(asAvatarStyle(user.avatar_style));
  const [seed, setSeed] = useState<string | null>(user.avatar_seed);
  const [url, setUrl] = useState<string | null>(user.avatar_url);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [savedAt, setSavedAt] = useState<Date | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  const preview = {
    userId: user.id,
    displayName: user.display_name,
    handle: user.handle,
    avatarUrl: url,
    avatarStyle: style,
    avatarSeed: seed,
  };

  async function onPickFile(e: React.ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;
    setError(null);
    try {
      setUrl(await fileToDataUrl(file));
    } catch (err) {
      setError((err as Error).message);
    }
  }

  async function save() {
    setSaving(true);
    setError(null);
    try {
      const updated = await setMyAvatar({ url, style, seed });
      onUpdated(updated);
      setSavedAt(new Date());
    } catch (err) {
      setError((err as unknown as ApiError).message);
    } finally {
      setSaving(false);
    }
  }

  return (
    <section className="space-y-4 border-t border-white/10 pt-6">
      <div className="space-y-1">
        <h2 className="text-lg font-semibold">Avatar.</h2>
        <p className="mono text-xs text-chrome-dim">
          a generated avatar by default — pick a style, reroll it, or upload your own.
        </p>
      </div>

      <div className="flex items-center gap-4">
        <Avatar size={64} user={preview} />
        <div className="mono text-xs text-chrome-dim">
          {url ? "uploaded image" : `generated · ${STYLE_LABEL[style]}`}
          <div className="text-[11px]">
            shown on cards, comments, and the nav — never the only signal.
          </div>
        </div>
      </div>

      {/* Style picker — each tile previews the same seed in that style. */}
      <div className="space-y-1.5">
        <span className="mono block text-xs uppercase tracking-widest text-chrome-dim">
          style
        </span>
        <div className="flex flex-wrap gap-2">
          {AVATAR_STYLES.map((s) => {
            const active = !url && s === style;
            return (
              <button
                type="button"
                key={s}
                onClick={() => {
                  setUrl(null);
                  setStyle(s);
                }}
                aria-pressed={active}
                title={STYLE_LABEL[s]}
                className={`flex items-center gap-2 rounded border px-2 py-1.5 text-xs transition ${
                  active
                    ? "border-accent bg-accent/10 text-chrome"
                    : "border-white/10 text-chrome-dim hover:border-white/20"
                }`}
              >
                <Avatar
                  size={22}
                  user={{ userId: user.id, avatarStyle: s, avatarSeed: seed }}
                />
                <span className="mono">{STYLE_LABEL[s]}</span>
              </button>
            );
          })}
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <button
          type="button"
          onClick={() => {
            setUrl(null);
            setSeed(randomSeed());
          }}
          className="mono rounded border border-white/10 px-3 py-1.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
        >
          $ regenerate
        </button>
        <button
          type="button"
          onClick={() => fileRef.current?.click()}
          className="mono rounded border border-white/10 px-3 py-1.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
        >
          upload image…
        </button>
        {url && (
          <button
            type="button"
            onClick={() => setUrl(null)}
            className="mono rounded border border-white/10 px-3 py-1.5 text-xs text-chrome-dim hover:border-white/20 hover:text-chrome"
          >
            use generated
          </button>
        )}
        <input
          ref={fileRef}
          type="file"
          accept="image/*"
          onChange={onPickFile}
          className="hidden"
          aria-label="upload avatar image"
        />
      </div>

      {error && (
        <div
          role="alert"
          className="mono rounded border border-red-500/30 bg-red-500/10 p-3 text-sm text-red-200"
        >
          {error}
        </div>
      )}

      <div className="flex items-center gap-4">
        <button
          type="button"
          onClick={save}
          disabled={saving}
          className="mono rounded bg-accent px-4 py-2 text-sm font-medium text-accent-fg transition hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
        >
          {saving ? "nudging electrons…" : "$ update avatar"}
        </button>
        {savedAt && (
          <span className="mono text-xs text-chrome-dim">
            saved {savedAt.toLocaleTimeString()}
          </span>
        )}
      </div>
    </section>
  );
}
