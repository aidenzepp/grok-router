---
name: error-messages
description: Write or review user-facing and developer-facing error messages. Use when adding validation errors, API errors, CLI errors, UI alerts, empty/error states, permission failures, quota errors, retry failures, or any copy that tells a user something went wrong.
---

# Error Messages

Use this skill to make errors clear, actionable, and appropriate to the audience.

Good error messages are not copy polish after the fact. First inspect the error boundary: what failed, who can fix it, what state is preserved, whether retry can work, and what structured details are available.

## Core Test

Every error should answer these questions as directly as the surface allows:

- What happened?
- Why did it happen, when known and safe to reveal?
- What can the user do next?
- What state was preserved or lost?
- Where can the user go if the primary fix does not work?

If the code cannot answer those questions, improve the error model before rewriting prose when scope allows.

## Workflow

### 1. Identify the audience

Decide which surface you are writing for:

- End-user UI: use plain product language and avoid implementation detail.
- Developer API: be concise, precise, and structured.
- CLI/log output: include enough context for search, automation, and debugging.
- Internal operator tooling: include identifiers, affected resource, and next diagnostic step.

Do not make one string serve every audience if the system has separate user, developer, log, and machine-readable surfaces.

### 2. Inspect the real failure

Before writing the message, find:

- the failing operation
- the invalid input or missing requirement
- the expected constraint or allowed range
- whether the failure is caused by the user, product, network, dependency, permission, quota, or incompatible state
- whether the user's data or progress was saved
- whether retry, changing input, changing permissions, contacting support, or waiting is actually useful

Do not offer "Try again" when the system can already tell retry will fail.

### 3. Write the message

Use this structure when the surface can support it:

```text
<what happened>
<why it happened or what constraint was violated>
<what to do next>
<fallback or support/debug identifier when useful>
```

For compact inline errors, collapse the structure into one sentence:

```text
Enter a 5-digit ZIP code
```

For blocking dialogs or API errors, include more context:

```text
Unable to connect your account
Your changes were saved, but we could not connect the account because the provider is unavailable. Try again later or contact support if this keeps happening.
```

### 4. Preserve state and blame carefully

- Say what was preserved: "Your changes were saved."
- Say what failed without shaming the user.
- Prefer instructions over accusations: "Enter a name" instead of "You did not enter a name."
- Avoid cute tone, humor, exclamation points, and excessive apologies.
- Use "we" sparingly; focus on the user's task and next action.

### 5. Make invalid input specific

When input is invalid, show both the offending value and the expected requirement when safe:

```text
The specified bid ($5) is below the minimum bid ($8)
```

For sensitive values, identify the field and constraint without echoing secrets:

```text
Password must include at least 12 characters
```

For long values, truncate or use progressive disclosure instead of flooding the user.

### 6. Make API errors structured

For APIs and machine callers, avoid making clients parse prose. Provide:

- stable error code or reason
- human-readable message
- field-level violations when applicable
- whole-error context such as resource, region, limit, actual value, retry time, draft ID, or support link when relevant
- item-specific details such as field violations, row violations, missing permissions, or quota violations

Keep dynamic facts in `details` or `context` if they also appear in the human message.

Prefer a small boundary payload unless the repo already has a stronger local convention:

```ts
type JsonValue =
  | string
  | number
  | boolean
  | null
  | JsonValue[]
  | { [key: string]: JsonValue };

type ErrorPayload<TDetail = JsonValue> = {
  code: string;
  message: string;
  details: TDetail[];
  context?: Record<string, JsonValue>;
};
```

Use the fields this way:

- `code`: stable machine-readable category.
- `message`: safe, audience-appropriate human text for this boundary.
- `details`: repeated or item-specific repair data, such as fields, rows, missing permissions, or violated limits.
- `context`: curated whole-error state, such as preserved draft IDs, affected resource, retry time, request ID, help URL, or available actions.

Do not add top-level fields such as `requestId`, `helpUrl`, or `actions` by default. Put optional, situation-specific facts in `context` unless the existing API contract already standardizes them.

Do not use `Record<string, unknown>` for public API payloads unless the repo already does. Prefer JSON-serializable values so the payload is deliberate, stable, and safe to expose.

For JavaScript and TypeScript internals, `Error.message` and `cause` are not enough for a product/API contract. Preserve native errors and causes internally, then normalize into `ErrorPayload` at the API, UI, CLI, or SDK boundary.

For Go internals, do not invent a universal application error type by default. Prefer ordinary errors, wrapping, `errors.Is`, `errors.As`, and small typed errors where callers need branching. Render those errors into the boundary payload or the repo's existing HTTP/gRPC error format at the edge.

Self-supporting multi-sentence messages belong where the code owns the final human surface: API response mapping, UI rendering, CLI output, SDK-facing errors, or operator tools. Deep domain, repository, storage, queue, crypto, or generic library code should usually return compact errors plus structured facts and causes.

### 7. Place and format the error

- Put field errors next to the field.
- Summarize multi-field form errors at the top and mark each field.
- Keep the user's input intact whenever possible.
- Use color plus text or icon cues; do not rely on color alone.
- Use progressive disclosure or docs links for long explanations.
- Keep terminology consistent across the product.

## Anti-Patterns

Avoid:

- `Something went wrong`
- `Unknown error`
- `Invalid input`
- `Permission denied`
- `Try again later` with no reason or fallback
- blame-shifting to the user, server, provider, or third party
- implementation jargon on end-user surfaces
- one generic catch-all for distinct failures with different fixes
- a human-readable sentence as the only machine-readable API contract

## Review Checklist

Before calling the error done, check:

- The message matches the actual condition.
- The next action is concrete and possible.
- Any invalid value, missing permission, limit, or constraint is named.
- The user is reassured about preserved data when relevant.
- The surface has an escape hatch: retry, edit, wait, docs, contact support, or error ID.
- API/CLI errors include stable identifiers for search and automation.
- The tone is neutral, respectful, concise, and not cute.
- The message is close to the error and accessible without color alone.

See `references/examples.md` for concrete rewrites and code patterns.
