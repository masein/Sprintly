# ADR 0002 — Outbound webhook delivery targets

**Status:** accepted · **Date:** 2026-06-14 · **Scope:** F2 (Slack/Discord adapters)

## Context

F2's first pass (#26) shipped generic signed delivery: a `webhooks` row holds
a URL + an encrypted secret, and the worker POSTs `{event, data}` with an
HMAC-SHA256 `X-Sprintly-Signature`. The deferred half is first-class
**Slack** and **Discord** targets, which differ from a generic endpoint in two
ways:

1. **Body shape.** Slack and Discord each want their own message JSON
   (`{text, blocks…}` / `{content, embeds…}`), not Sprintly's `{event, data}`.
2. **Auth.** An incoming Slack/Discord webhook URL *is* the secret — there's
   no signature to compute; possession of the URL is the credential.

## Decision

A delivery **target type** is a small enum on the existing `webhooks` row:
`target_type ∈ {outbound, slack, discord}`, default `outbound`. Formatting and
auth are chosen **at delivery time** in the worker, keyed off that column:

- **outbound** — body `{event, data}`, HMAC-signed with the row's secret,
  `X-Sprintly-*` headers (unchanged behaviour). A secret is required.
- **slack / discord** — body built by a pure formatter
  (`domain::chat_adapters`) from the neutral `(event, data)`; POST to the URL
  with `Content-Type: application/json` only. No secret, no signature.

`dispatch` stays target-agnostic: it enqueues `deliver_webhook` jobs carrying
the raw `(event, data)` and filters to *deliverable* rows (an outbound row
needs a secret; a chat row needs only its URL). The worker reconstructs the
body per target type — so the same job pipeline, retry/backoff, and
`webhook_deliveries` audit serve all three.

## Alternatives considered

- **Separate `webhook_adapters` table / distinct job kinds per target.** More
  moving parts for what is one column's worth of variation; the delivery
  pipeline (claim → POST → record → backoff) is identical across targets, so
  forking it buys nothing.
- **Format at dispatch time** (precompute the body, as today). Dispatch would
  need to branch per subscriber and the job payload would carry provider-shaped
  bytes. Keeping `(event, data)` in the job and formatting at delivery keeps
  dispatch simple and makes the formatters trivially unit-testable.
- **`trait DeliveryTarget`.** Three known targets, all pure formatting; an enum
  `match` is shorter and monomorphic (mirrors ADR 0001).

## Consequences

- Adding a target = one enum variant + one pure formatter + a UI option.
- Chat rows carry no secret; the deliverability filter and the worker both
  treat "outbound without secret" as not-yet-configured (dropped), unchanged.
- "Send test" reuses the same path with a synthetic `(event, data)`, so a
  misconfigured Slack URL surfaces in `webhook_deliveries` like any failure.
- Formatters are intentionally minimal (a one-line summary + a task link);
  richer blocks/embeds can grow inside the adapter without touching delivery.
