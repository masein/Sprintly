"use client";

// /me/achievements — catalog with "earned" decoration on the rows the user has.

import { useQuery } from "@tanstack/react-query";
import { useRouter } from "next/navigation";
import { Trophy } from "lucide-react";
import { AppShell } from "@/components/AppShell";
import { Sprint } from "@/components/Sprint";
import {
  listCatalog,
  listMyAchievements,
  type AwardedRow,
  type CatalogRow,
} from "@/lib/achievements";
import type { ApiError } from "@/lib/api";

export default function AchievementsPage() {
  const router = useRouter();
  const catalog = useQuery({ queryKey: ["achievements-catalog"], queryFn: listCatalog });
  const mine = useQuery({ queryKey: ["my-achievements"], queryFn: listMyAchievements });

  if (catalog.error) {
    const e = catalog.error as unknown as ApiError;
    if (e.status === 401) {
      router.push("/login");
      return null;
    }
  }

  const earnedByCode = new Map<string, AwardedRow>();
  for (const r of mine.data ?? []) earnedByCode.set(r.code, r);

  const all = catalog.data ?? [];
  const earnedCount = all.filter((c) => earnedByCode.has(c.code)).length;

  return (
    <AppShell>
      <header className="mb-6 flex items-end gap-4">
        <Sprint mood={earnedCount > 0 ? "proud" : "neutral"} size={64} />
        <div>
          <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
            sprintly · achievements
          </div>
          <h1 className="text-3xl font-semibold">
            {earnedCount} of {all.length} earned.
          </h1>
          <p className="mt-1 text-sm text-chrome-dim">
            None of these reward longer hours. Promise.
          </p>
        </div>
      </header>

      <ul className="grid grid-cols-1 gap-3 md:grid-cols-2">
        {all.map((c) => (
          <Row key={c.code} catalog={c} earned={earnedByCode.get(c.code)} />
        ))}
      </ul>
    </AppShell>
  );
}

function Row({
  catalog,
  earned,
}: {
  catalog: CatalogRow;
  earned: AwardedRow | undefined;
}) {
  const yes = !!earned;
  return (
    <li
      className={`rounded-lg border p-3 ${
        yes
          ? "border-accent/40 bg-accent/5"
          : "border-white/10 bg-ink-subtle opacity-70"
      }`}
    >
      <div className="flex items-center gap-2">
        <Trophy size={14} className={yes ? "text-accent" : "text-chrome-dim"} />
        <span className={`text-sm ${yes ? "text-chrome" : "text-chrome-dim"}`}>
          {catalog.title}
        </span>
        {yes && (
          <span className="mono ml-auto rounded border border-accent/40 px-1.5 py-0.5 text-[10px] uppercase tracking-widest text-accent">
            earned · {earned!.awarded_at.slice(0, 10)}
          </span>
        )}
      </div>
      <p className="mono mt-1 text-[11px] text-chrome-dim">{catalog.description}</p>
    </li>
  );
}
