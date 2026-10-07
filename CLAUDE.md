@AGENTS.md

Before you start a task, invoke the skill that matches it, so the task runs at the right effort level: `quick-task`
(medium), `ui-change` (high), `review-core` or `detector-work` (xhigh), `wrong-result` (max). Skip this only for a reply
that needs no tool. The effort lasts one turn, so invoke it again when the work goes on. Scope can raise the pick: work
across the core, the service and the UI takes the highest of their skills; anything the parity tests or the stats-file
checks judge is never `quick-task`; an unknown cause is `wrong-result`; a new feature across modules gets a plan and the
user's yes first.

Hand sub-tasks to the agent at their level, in the background, so nothing waits on them: `scout` (haiku, low) for a
search across many files or a big log, `runner` (haiku, low) for a known slow command, `mechanic` (sonnet, medium) for
a clear rule applied over many files, `builder` (high) for a settled piece of a feature with its test. Keep a lookup of
one or two files here, and keep judgment (design, causes, results) here. A command `COSTS.md` puts over 30 s runs in the
background.
