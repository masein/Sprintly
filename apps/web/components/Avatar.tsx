// A user avatar: an uploaded image when one is set, otherwise a deterministic
// generated avatar (see lib/avatar). Always carries an accessible label, and is
// always paired with the @handle / display name in its surface — never the only
// signal for who someone is.

import { avatarSvg, asAvatarStyle } from "@/lib/avatar";

export type AvatarUser = {
  /** Stable id used as the default generated seed. */
  userId: string;
  displayName?: string | null;
  handle?: string | null;
  avatarUrl?: string | null;
  avatarStyle?: string | null;
  avatarSeed?: string | null;
};

export function Avatar({
  user,
  size = 24,
  className = "",
}: {
  user: AvatarUser;
  size?: number;
  className?: string;
}) {
  const label =
    user.displayName?.trim() ||
    (user.handle ? `@${user.handle}` : "user");
  const dim = { width: size, height: size } as const;

  // A ring + rounded frame so the avatar reads on any theme background.
  const frame =
    "inline-block shrink-0 overflow-hidden rounded-full ring-1 ring-white/10 bg-ink-subtle";

  if (user.avatarUrl) {
    return (
      // eslint-disable-next-line @next/next/no-img-element
      <img
        src={user.avatarUrl}
        alt=""
        role="img"
        aria-label={label}
        style={dim}
        className={`${frame} object-cover ${className}`}
      />
    );
  }

  const seed = user.avatarSeed?.trim() || user.userId;
  const svg = avatarSvg(seed, asAvatarStyle(user.avatarStyle));
  return (
    <span
      role="img"
      aria-label={label}
      style={dim}
      className={`${frame} ${className}`}
      dangerouslySetInnerHTML={{ __html: svg }}
    />
  );
}
