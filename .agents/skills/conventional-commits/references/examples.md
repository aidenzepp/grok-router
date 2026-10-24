# Conventional Commit Examples

These hypothetical scenarios teach commit boundaries and messages, not application implementation. File lists describe the complete contents of each proposed commit. Git commands are excerpts for the stated working-tree and index conditions; do not run them against an unrelated checkout.

## Choose the most specific truthful prefix

Scenario: a documentation-only change clarifies local setup in `README.md`. It changes no runtime behavior, tooling, or configuration.

### POSITIVE — Name the documentation change

Decisive evidence: `docs` identifies the actual change more specifically than the repository-maintenance category; the subject states its concrete purpose.

```text
docs: clarify local setup instructions
```

### NEGATIVE — Use a broader category despite a specific fit

Decisive evidence: the same subject remains accurate, but `chore` obscures that this commit changes documentation only.

```text
chore: clarify local setup instructions
```

## Write a concrete, imperative subject

Scenario: a bug fix restores the health endpoint's documented JSON content type. The commit includes the handler change and its regression test.

### POSITIVE — State the behavior restored

Decisive evidence: the imperative verb and named behavior let a reviewer understand the fix without opening the diff.

```text
fix(api): restore the health response JSON content type
```

### NEGATIVE — Hide the behavior behind a generic verb

Decisive evidence: the prefix and scope are correct and the verb is imperative, but “update” does not identify what was broken or what the fix restores.

```text
fix(api): update the health response
```

## Split independent stories and retain their proof

Scenario: the working tree contains the health content-type fix, its regression test, and an unrelated clarification of setup instructions. Both stories pass their relevant checks independently. The following lists show complete proposed commit boundaries, not shell commands.

### POSITIVE — Keep the fix and regression test together

Decisive evidence: the first commit includes both the restored behavior and its proof; the unrelated documentation change has its own independently useful boundary.

```text
Commit 1: fix(api): restore the health response JSON content type
  apps/api/src/healthz/handlers.ts — fix the response
  apps/api/src/worker.test.ts — verify its JSON content type

Commit 2: docs: clarify local setup instructions
  README.md — clarify setup
```

### NEGATIVE — Combine an unrelated story with the fix

Decisive evidence: the fix still includes its regression test and its message is accurate, but the setup clarification is unrelated to the behavior this commit restores.

```text
Commit 1: fix(api): restore the health response JSON content type
  apps/api/src/healthz/handlers.ts — fix the response
  apps/api/src/worker.test.ts — verify its JSON content type
  README.md — clarify setup
```

## Stage only the intended boundary

Scenario: the index is empty. The three tracked files from the preceding scenario are modified, each contains only its stated change, and there are no untracked files. The complete working-tree diff has already been inspected. The intended next commit is the health fix and its regression test; setup documentation belongs in a later commit. Commands run from the repository root and stop before committing.

### POSITIVE — Stage the two files belonging to the fix

Decisive evidence: explicit paths select exactly the intended fix and its proof; inspecting the staged diff confirms the boundary before a commit is created.

```bash
git add -- apps/api/src/healthz/handlers.ts apps/api/src/worker.test.ts
git diff --cached --stat
git diff --cached
```

### NEGATIVE — Stage an inspected but unrelated change too

Decisive evidence: every changed file has been inspected, but the explicit staging list still includes `README.md`, which belongs to the separate documentation story. Inspecting the staged diff does not make that boundary correct.

```bash
git add -- apps/api/src/healthz/handlers.ts apps/api/src/worker.test.ts README.md
git diff --cached --stat
git diff --cached
```

When one file contains changes for multiple stories, select the intended hunks with `git add -p -- <path>` and inspect the staged diff. Do not stage the whole file merely because its path belongs to the feature.

## Add a body when it explains a consequential choice

Scenario: a CI change serializes preview deployment and cleanup in the same concurrency group. The diff shows the configuration; the body explains why an older in-flight deployment must finish before cleanup. This scenario does not prescribe changing the template's current workflow.

### POSITIVE — Explain the reason behind the configuration

Decisive evidence: the body supplies a causal constraint that the concurrency setting alone does not express, without repeating the subject or claiming unperformed verification.

```text
ci: serialize preview deployment and cleanup

Cleanup must wait for an in-flight deployment to finish; otherwise that
deployment can recreate resources after cleanup has removed them.
```

### NEGATIVE — Add a body that only repeats the subject

Decisive evidence: the subject and category remain correct, but the body adds no rationale, scope boundary, trade-off, or verification evidence.

```text
ci: serialize preview deployment and cleanup

Serialize preview deployment and cleanup in CI.
```

For a self-explanatory change such as correcting a broken documentation link, use a subject alone. Do not add a body solely to fill a template, and report checks as passed only when they actually ran and passed.
