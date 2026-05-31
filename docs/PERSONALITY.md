# Personality

> The voice and the visuals. This is not decoration — it's a feature. M9
> formalises it. This file is the source of truth from day one so we don't
> accumulate cringey copy in the meantime.

## Voice in one sentence

A senior engineer with good taste, dry humour, and somewhere else to be.

## Do

- Speak like a human who reads pull requests.
- Use developer jargon comfortably. `git fetch`, `ssh`, off-by-one, segfault.
- Be briefly funny in micro-moments, then get out of the way.
- Default to dark mode. Inter for UI. JetBrains Mono for IDs, timestamps,
  code, anything in the command palette.

## Don't

- "Hey there, partner!", "Yay!", or any exclamation-mark frosting.
- Patronise the user. Never explain what a sprint is in product copy.
- Fake AI personas. No "Sprintly suggests…" with no Sprintly behind it.
- Use color as the only signal. Pair it with shape, label, or icon.

## Required micro-copy patterns

| Surface             | Pattern                                                        |
| ------------------- | -------------------------------------------------------------- |
| Loading states      | "git fetch --rebase your-stuff" · "compiling vibes…" · "nudging electrons…" |
| Empty boards        | "Either you shipped everything or you're really good at scope creep." |
| Empty inbox         | "Inbox zero. Touch grass."                                     |
| Empty timesheet     | "Either you didn't work this week or you forgot the timer. Both are valid." |
| 500 page            | "We broke it. The trace ID is `…`. Tell an admin and go get coffee."     |
| 404 page            | "This page is in a different branch."                          |

## Mascot

A small pixelated beaver in glasses. Name: **Sprint**. Seven moods, that's it.
Appears in:

- empty states
- the boot screen
- confetti on sprint close

On April 1st, the mascot wears sunglasses. That's the whole joke.

## Achievements (samples)

`BUG_SLAYER`, `PR_WIZARD`, `ESTIMATOR_SUPREME`, `WATCHER_IN_THE_WHEAT_FIELD`,
`COFFEE_ADDICT`, `SPRINT_CLOSER`, `RETRO_HERO`, `RTFM`. Awarded via background
job, surfaced via toast + avatar badge.

**Never reward longer hours.** `COFFEE_ADDICT` is a wink, not a leaderboard.
The Coffee Meter is for the user, not their manager — managers do not see
other people's meters.

## Easter eggs

- `konami` in the command palette → retro CRT mode for 10 minutes.
- `sudo …` → "Permission denied — you are not in the sudoers file."
- `rm -rf` → modal: "Nice try."
- `:q` closes a modal; `:wq` saves & closes.
- `kudos @handle thanks for the migration` files a retro note in the current
  sprint.

## Confetti budget

Confetti fires on, and only on:

- closing a sprint
- completing a P0
- earning an achievement

Anything else is too much.

## Sound

Off by default. Subtle clack on card move, tiny chime on achievement. Mutable
globally and per-event.

## Themes (M9)

Midnight (default), Daylight, Solarized Dusk, Terminal Green, Hot Pink. All
expressed as CSS variables — components never hardcode colors.
