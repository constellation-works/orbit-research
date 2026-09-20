---
type: design
summary: "Central terminal styling without conflating scientific and execution status."
---

# Spec: Color and Styling

Styling is optional emphasis chosen by the sink and renderer. Removing every
ANSI sequence must lose no information or scientific distinction.

## Why This Exists

Research status and Orbit run status represent different facts. Copying Orbit's
success/error palette directly could suggest that a completed run supports a
hypothesis. The [payload decision](../4_decisions.md#presentation-must-not-alter-the-scientific-payload)
forbids that inference.

## 1. Current Vocabulary

Headers may be dim; all record values currently remain neutral text. Do not
introduce a semantic palette until its domain mapping is defined in one place.
Unrecognized future statuses must remain neutral, never cause a render failure.

## 2. Emission Policy

Only human tables on stdout TTY may contain styling. Nonempty `NO_COLOR` or
`TERM=dumb` disables it. Plain pipes, explicit table pipes, JSON and NDJSON never
contain styling, even with `CLICOLOR_FORCE`. All checks live in `output/sink.rs`.
Clap help is textual process output, separate from payload styling.

## 3. Rules for Future Styling

Use basic ANSI attributes and the terminal's palette; no background washes,
blinking or whole-row severity. Keep status words visible. Do not paint a column
uniformly merely because the user filtered on that value. Domain-to-role mapping
must distinguish scientific assessment from task/run state. No styling fields
belong in machine payloads.

## 4. Accessibility

Color must never carry the sole distinction. Dim is optional structure, not the
only way to identify a field. Contrast on users' terminal themes has not been
validated; honor `NO_COLOR` without reducing available information.
