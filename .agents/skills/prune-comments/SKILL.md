---
name: prune-comments
description: Prune redundant code comments and docstrings while preserving useful rationale, contracts, and tool directives. Use at the end of every task in this repository and whenever the user invokes $prune-comments or asks to clean up comments.
---

# Prune Comments

Remove commentary that merely narrates clear code. Preserve information a reader
cannot readily recover from names, types, and the nearby implementation. Apply
the same judgment to inline comments, block comments, documentation comments,
and docstrings; public visibility alone does not make repetitive prose useful.

## Scope and timing

Before the final handoff of every task, review comments added or edited during
the task and existing comments in the code touched by it. Complete this pass
before committing or publishing the finished changes. For tasks without code
changes, the pass is a no-op; do not manufacture unrelated cleanup.

When explicitly summoned, use the files or area the user requests. If no area
is specified, review the current task's code changes or the working diff. If
neither provides a scope, ask which code to review rather than pruning the
entire repository.

## What to prune

- Line-by-line narration, obvious section labels, and paraphrases of function,
  variable, or type names.
- Documentation that only restates a transparent wrapper or simple branch.
- Repeated explanations and boilerplate that add no contract or rationale.
- Stale or misleading prose, after checking the intended behavior. If it
  describes a still-relevant constraint, correct it instead of discarding it.

For example, `chip_id()` forwarding to
`self.read_register(REG_CHIP_ID).await` needs no prose saying it reads the device
identity register or repeats the obvious bus-error propagation. Likewise,
`verify_identity()` comparing that value with `EXPECTED_CHIP_ID` needs no prose
narrating the comparison and its directly visible error branches. Remove those
doc blocks unless a required documentation lint needs a minimal portion.

## What to keep

- Reasons for a design choice, tradeoffs, and explanations of surprising code.
- Hardware quirks, protocol constraints, timing requirements, and workarounds
  with relevant source or issue references.
- Contracts that callers need without reading implementation: units, coordinate
  frames, valid ranges, side effects, ordering, cancellation behavior, and
  non-obvious failure or panic conditions.
- Useful API examples and module-level context that explain how to use code.
- Safety rationale, license notices, attribution, and meaningful TODOs with
  actionable context.
- Tool directives and functional comments, including lint/type suppressions,
  formatter controls, coverage pragmas, doctests, and generated-code markers.

When a comment mixes useful context with narration, shorten it to the useful
part. Keep uncertain rationale until its purpose is understood. Do not replace
every removed comment with another comment or enforce a comment-count target.

## Review and verification

Read each candidate with its surrounding code; do not strip comments with a
blanket regex or rewrite code to justify deleting documentation. Preserve
runtime behavior, including docstrings consumed by application code or tools.
Leave generated and vendored files to their source workflow.

Inspect the final diff for lost context, dangling documentation headings, and
unrelated edits. Honor required documentation lints with the shortest useful
documentation; do not disable lints or add suppression attributes to permit
deletions. Run formatting and relevant lint/documentation checks when affected.
Reuse valid checks already run if this pass makes no further changes. Mention
material pruning in the handoff; a no-op does not need a separate report.
