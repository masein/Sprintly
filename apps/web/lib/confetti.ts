// DIY confetti. Lightweight. No dependency. Per spec §10:
//
//   "Confetti is allowed only on: closing a sprint, completing a P0,
//    getting an achievement. Anything else is too much."
//
// We do not enforce that at the call site — discipline at the caller. This
// module just gives you `fire()` and gets out of the way.

const COLORS = ["#7c5cff", "#22d3ee", "#10b981", "#f59e0b", "#ef4444", "#ec4899"];

export function fire(count = 80): void {
  if (typeof document === "undefined") return;
  const root = document.body;
  for (let i = 0; i < count; i++) {
    const piece = document.createElement("div");
    piece.className = "confetti-piece";
    const color = COLORS[Math.floor(Math.random() * COLORS.length)]!;
    piece.style.background = color;
    piece.style.left = `${Math.random() * 100}%`;
    piece.style.animationDuration = `${1.6 + Math.random() * 1.4}s`;
    piece.style.transform = `rotate(${Math.random() * 360}deg)`;
    root.appendChild(piece);
    // Remove after animation so we don't leak DOM nodes on repeat fires.
    setTimeout(() => piece.remove(), 3500);
  }
}
