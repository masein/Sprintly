"use client";

// Sprint. The mascot. A pixelated beaver in glasses.
//
// Implementation note: pixel art via a discrete 16×16 grid rendered as SVG
// rects. Colors are theme-aware via `currentColor` + the mood palette.
// Seven moods. On April 1st (server-and-client local), sunglasses overlay
// replaces the regular glasses. That's the whole joke.
//
// No external assets. The whole mascot fits in this file.

import { useMemo } from "react";

export type Mood = "happy" | "neutral" | "sad" | "surprised" | "smug" | "proud" | "sleepy";

const PIXEL = 8;
const GRID = 16;
const SIZE = PIXEL * GRID; // 128px

type Palette = {
  fur: string;
  furDark: string;
  tooth: string;
  glass: string;
};

const PALETTE: Palette = {
  fur: "#a16207",        // saddle brown
  furDark: "#78350f",
  tooth: "#fef3c7",
  glass: "#000000",
};

// Each mood lays out the eyes + mouth on the face grid. We start with a
// stable body grid and overlay the mood-specific cells. `.` = empty, `f` =
// fur, `d` = furDark, `t` = tooth, `g` = glass.
const BODY: string[] = [
  "................",
  "................",
  "....ffffffff....",
  "...ffffffffff...",
  "..ffffffffffff..",
  "..ffffffffffff..",
  "..fff......fff..",  // glasses zone
  "..fff......fff..",  // (mood overlay paints here)
  "..ffffffffffff..",
  "..ffff....ffff..",  // mouth zone
  "..ffffffffffff..",
  "..ffffffffffff..",
  "...ffffffffff...",
  "....ffffffff....",
  ".....dd..dd.....",
  ".....dd..dd.....",
];

// Per-mood eye + mouth overlays.
// Each entry is (row, col, char).
const OVERLAYS: Record<Mood, [number, number, string][]> = {
  happy: [
    [6, 4, "g"], [6, 5, "g"], [6, 10, "g"], [6, 11, "g"],
    [7, 4, "g"], [7, 5, "g"], [7, 10, "g"], [7, 11, "g"],
    [9, 6, "t"], [9, 7, "t"], [9, 8, "t"], [9, 9, "t"],
  ],
  neutral: [
    [6, 4, "g"], [6, 5, "g"], [6, 10, "g"], [6, 11, "g"],
    [7, 4, "g"], [7, 5, "g"], [7, 10, "g"], [7, 11, "g"],
    [9, 7, "t"], [9, 8, "t"],
  ],
  sad: [
    [6, 4, "g"], [6, 5, "g"], [6, 10, "g"], [6, 11, "g"],
    [7, 4, "g"], [7, 5, "g"], [7, 10, "g"], [7, 11, "g"],
    [10, 6, "d"], [10, 7, "t"], [10, 8, "t"], [10, 9, "d"],
  ],
  surprised: [
    [5, 4, "g"], [5, 5, "g"], [5, 10, "g"], [5, 11, "g"],
    [6, 4, "g"], [6, 5, "g"], [6, 10, "g"], [6, 11, "g"],
    [7, 4, "g"], [7, 5, "g"], [7, 10, "g"], [7, 11, "g"],
    [9, 7, "t"], [9, 8, "t"], [10, 7, "t"], [10, 8, "t"],
  ],
  smug: [
    [6, 4, "g"], [6, 5, "g"], [6, 10, "g"], [6, 11, "g"],
    [7, 5, "g"], [7, 10, "g"],
    [9, 5, "t"], [9, 6, "t"], [9, 7, "t"], [9, 8, "t"], [9, 9, "t"],
  ],
  proud: [
    [6, 4, "g"], [6, 5, "g"], [6, 10, "g"], [6, 11, "g"],
    [7, 4, "g"], [7, 5, "g"], [7, 10, "g"], [7, 11, "g"],
    [9, 5, "t"], [9, 6, "t"], [9, 7, "t"], [9, 8, "t"], [9, 9, "t"], [9, 10, "t"],
  ],
  sleepy: [
    [6, 4, "d"], [6, 5, "d"], [6, 10, "d"], [6, 11, "d"],
    [7, 4, "d"], [7, 5, "d"], [7, 10, "d"], [7, 11, "d"],
    [9, 7, "t"], [9, 8, "t"],
  ],
};

function isApril1(): boolean {
  if (typeof window === "undefined") return false;
  const d = new Date();
  return d.getMonth() === 3 && d.getDate() === 1; // April = month index 3
}

export function Sprint({
  mood = "happy",
  size = SIZE,
  className,
}: {
  mood?: Mood;
  size?: number;
  className?: string;
}) {
  const sunglasses = useMemo(isApril1, []);

  // Compose the grid: start with body, apply mood overlay; if sunglasses,
  // paint a black bar across the eye row.
  const grid = useMemo(() => {
    const g = BODY.map((row) => row.split(""));
    for (const [r, c, ch] of OVERLAYS[mood]) {
      g[r][c] = ch;
    }
    if (sunglasses) {
      // Black bar across both eye rows (rows 6+7, cols 3-12).
      for (let r = 6; r <= 7; r++) {
        for (let c = 3; c <= 12; c++) {
          g[r][c] = "g";
        }
      }
    }
    return g;
  }, [mood, sunglasses]);

  return (
    <svg
      viewBox={`0 0 ${GRID * PIXEL} ${GRID * PIXEL}`}
      width={size}
      height={size}
      className={className}
      role="img"
      aria-label={`Sprint the beaver (${mood}${sunglasses ? ", sunglasses" : ""})`}
      shapeRendering="crispEdges"
    >
      {grid.map((row, r) =>
        row.map((cell, c) => {
          const color =
            cell === "f" ? PALETTE.fur :
            cell === "d" ? PALETTE.furDark :
            cell === "t" ? PALETTE.tooth :
            cell === "g" ? PALETTE.glass :
            null;
          if (!color) return null;
          return (
            <rect
              key={`${r}-${c}`}
              x={c * PIXEL}
              y={r * PIXEL}
              width={PIXEL}
              height={PIXEL}
              fill={color}
            />
          );
        }),
      )}
    </svg>
  );
}
