# Dashboard panels

The plugin adds four read-only panels to Orbit's Plugins tab for the workspace. Use
them to see what needs `accept` or `assess` next.

| Panel | Shows |
|---|---|
| Open questions | Open questions with their tags and linked tasks |
| Results awaiting acceptance | Results delivered by a run and not accepted |
| Hypotheses and assessments | Each hypothesis revision with the latest verdict per result; disagreements keep separate rows and mark the revision disputed |
| Corpus health | Validity, base revision and record counts by kind and status |

A table shows at most 200 rows and ends with a row naming the `research list` command
for the rest. A workspace without a corpus shows a short explanation instead: a research
corpus needs its own new, empty directory made with `orbit-research workspace init <dir>`
and registered as an Orbit workspace, because the plugin reads the workspace root as
the corpus.

## Results awaiting acceptance

A result is listed when it has no acceptance artifact yet, when its README changed
since it was accepted, or as `acceptance unknown` when the acceptance cannot be checked
right now, with a reason such as `orbit not available`, `orbit call timed out` or
`artifact unreadable`. A result with no `orbit.task` and `orbit.run`, such as one
investigated outside a job, is not listed.

The panel calls Orbit once or twice per delivered result, each call cut off after 5
seconds and the whole panel after 20, and counts a result as accepted only by the check
`assess` applies: the full acceptance artifact, naming that result and its current
README. It never guesses.

One deliberate exception: once a live read confirmed an acceptance, the panel keeps a
small file for that exact (result, task, README) under
`_data/orbit-research-operations/acceptance/` in a prepared corpus and hides the row on
later refreshes without calling Orbit, even while Orbit is unreachable, because an
acceptance is immutable and an edited README no longer matches. That cache is trusted
local state, like the corpus: it can only hide a row, and `assess` never reads it.
