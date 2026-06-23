// Deterministic, self-contained generated avatars.
//
// Everything here is a pure function of (seed, style): the same inputs always
// produce byte-identical SVG, and nothing ever touches the network — the markup
// is inlined, so it works offline and on air-gapped self-hosted installs.
//
// Styles, in keeping with docs/PERSONALITY.md (pixelated beaver mascot "Sprint",
// dry not cutesy):
//   beaver     — a small pixel critter in glasses, the mascot vibe
//   robot      — a pixel robot face
//   identicon  — a symmetric 5×5 block identicon
//   glyph      — a single curated emoji on a tinted tile (the "emoji" option)

export type AvatarStyle = "beaver" | "robot" | "identicon" | "glyph";

export const AVATAR_STYLES: AvatarStyle[] = [
  "beaver",
  "robot",
  "identicon",
  "glyph",
];

export const DEFAULT_AVATAR_STYLE: AvatarStyle = "beaver";

// ── seeded randomness ────────────────────────────────────────────────────────
// xmur3 string hash → 32-bit seed, then mulberry32 for a small, fast,
// deterministic PRNG. Both are standard public-domain snippets.

function xmur3(str: string): number {
  let h = 1779033703 ^ str.length;
  for (let i = 0; i < str.length; i++) {
    h = Math.imul(h ^ str.charCodeAt(i), 3432918353);
    h = (h << 13) | (h >>> 19);
  }
  h = Math.imul(h ^ (h >>> 16), 2246822507);
  h = Math.imul(h ^ (h >>> 13), 3266489909);
  return (h ^= h >>> 16) >>> 0;
}

function mulberry32(a: number): () => number {
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

type Rng = () => number;

function rngFor(seed: string, salt: string): Rng {
  return mulberry32(xmur3(`${salt}:${seed}`));
}

function pick<T>(rng: Rng, items: readonly T[]): T {
  return items[Math.floor(rng() * items.length)]!;
}

// A vivid-but-not-garish tile hue, plus a pale ink that reads on top of it.
// Saturation/lightness are clamped so no seed produces neon or mud.
function palette(rng: Rng): { tile: string; ink: string; deep: string } {
  const hue = Math.floor(rng() * 360);
  return {
    tile: `hsl(${hue} 58% 52%)`,
    ink: `hsl(${hue} 45% 92%)`,
    deep: `hsl(${hue} 50% 24%)`,
  };
}

// ── style renderers ──────────────────────────────────────────────────────────
// Each renderer draws inside a 0 0 100 100 viewBox on a rounded tile.

function tile(fill: string): string {
  return `<rect width="100" height="100" rx="22" fill="${fill}"/>`;
}

function beaver(seed: string): string {
  const rng = rngFor(seed, "beaver");
  const p = palette(rng);
  const earR = 9 + Math.floor(rng() * 4);
  const eyeGap = 13 + Math.floor(rng() * 5);
  const cx = 50;
  return [
    tile(p.tile),
    // ears
    `<circle cx="${cx - 20}" cy="28" r="${earR}" fill="${p.deep}"/>`,
    `<circle cx="${cx + 20}" cy="28" r="${earR}" fill="${p.deep}"/>`,
    // face
    `<rect x="26" y="26" width="48" height="46" rx="16" fill="${p.ink}"/>`,
    // glasses (the whole joke — the mascot wears them)
    `<line x1="${cx - eyeGap}" y1="44" x2="${cx + eyeGap}" y2="44" stroke="${p.deep}" stroke-width="3"/>`,
    `<circle cx="${cx - eyeGap}" cy="44" r="6" fill="none" stroke="${p.deep}" stroke-width="3"/>`,
    `<circle cx="${cx + eyeGap}" cy="44" r="6" fill="none" stroke="${p.deep}" stroke-width="3"/>`,
    `<circle cx="${cx - eyeGap}" cy="44" r="2.4" fill="${p.deep}"/>`,
    `<circle cx="${cx + eyeGap}" cy="44" r="2.4" fill="${p.deep}"/>`,
    // nose + two front teeth
    `<rect x="${cx - 4}" y="54" width="8" height="6" rx="2" fill="${p.deep}"/>`,
    `<rect x="${cx - 5}" y="60" width="4" height="8" rx="1" fill="#fff"/>`,
    `<rect x="${cx + 1}" y="60" width="4" height="8" rx="1" fill="#fff"/>`,
  ].join("");
}

function robot(seed: string): string {
  const rng = rngFor(seed, "robot");
  const p = palette(rng);
  const square = rng() > 0.5;
  const eye = (x: number) =>
    square
      ? `<rect x="${x - 6}" y="40" width="12" height="12" rx="2" fill="${p.deep}"/>`
      : `<circle cx="${x}" cy="46" r="6.5" fill="${p.deep}"/>`;
  const teeth = 3 + Math.floor(rng() * 3);
  const mouth = Array.from({ length: teeth }, (_, i) => {
    const w = 44 / teeth;
    return `<rect x="${28 + i * w + 1}" y="62" width="${w - 2}" height="8" rx="1" fill="${
      rng() > 0.4 ? p.deep : p.tile
    }"/>`;
  }).join("");
  return [
    tile(p.tile),
    // antenna
    `<line x1="50" y1="14" x2="50" y2="24" stroke="${p.deep}" stroke-width="3"/>`,
    `<circle cx="50" cy="12" r="4" fill="${p.deep}"/>`,
    // head
    `<rect x="24" y="24" width="52" height="52" rx="10" fill="${p.ink}"/>`,
    eye(40),
    eye(60),
    `<rect x="26" y="60" width="48" height="12" rx="3" fill="${p.ink}"/>`,
    mouth,
  ].join("");
}

function identicon(seed: string): string {
  const rng = rngFor(seed, "identicon");
  const p = palette(rng);
  const cells: string[] = [];
  const unit = 100 / 5;
  for (let row = 0; row < 5; row++) {
    for (let col = 0; col < 3; col++) {
      if (rng() > 0.5) {
        for (const c of [col, 4 - col]) {
          cells.push(
            `<rect x="${c * unit}" y="${row * unit}" width="${unit}" height="${unit}" fill="${p.ink}"/>`,
          );
        }
      }
    }
  }
  return [tile(p.deep), ...cells].join("");
}

// Curated, deliberately non-human set — objects and critters only.
const GLYPHS = [
  "🦫",
  "🚀",
  "🛰️",
  "🦊",
  "🐙",
  "🦉",
  "🌵",
  "🍄",
  "🎲",
  "🧩",
  "🛠️",
  "⚡",
  "🌀",
  "🔭",
  "🧭",
  "🪐",
];

function glyph(seed: string): string {
  const rng = rngFor(seed, "glyph");
  const p = palette(rng);
  const g = pick(rng, GLYPHS);
  return [
    tile(p.tile),
    `<text x="50" y="50" font-size="52" text-anchor="middle" dominant-baseline="central">${g}</text>`,
  ].join("");
}

const RENDERERS: Record<AvatarStyle, (seed: string) => string> = {
  beaver,
  robot,
  identicon,
  glyph,
};

/**
 * Deterministic avatar markup for a seed + style. Returns a complete `<svg>`
 * string (sized 100%); the same arguments always return the identical string.
 */
export function avatarSvg(seed: string, style: AvatarStyle = DEFAULT_AVATAR_STYLE): string {
  const render = RENDERERS[style] ?? RENDERERS[DEFAULT_AVATAR_STYLE];
  return (
    `<svg viewBox="0 0 100 100" width="100%" height="100%" ` +
    `xmlns="http://www.w3.org/2000/svg" aria-hidden="true" focusable="false">` +
    render(seed || "anon") +
    `</svg>`
  );
}

/** Coerce an arbitrary stored value to a known style (or the default). */
export function asAvatarStyle(v: string | null | undefined): AvatarStyle {
  return AVATAR_STYLES.includes(v as AvatarStyle)
    ? (v as AvatarStyle)
    : DEFAULT_AVATAR_STYLE;
}
