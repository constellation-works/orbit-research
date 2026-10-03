#!/bin/sh
# The one live smoke run of the v1 research loop.
#
# Run it once, by hand, on the host where the `research` Orbit plugin is
# installed and enabled, with a real crew. It drives a disposable corpus through
# the loop the deterministic end-to-end test (crates/orbit-research-cli/tests/
# e2e_v1_loop.rs) proves with a scripted agent, but this time the investigation
# is done by a real provider and costs real money:
#
#   capture a small question -> hypothesis -> reserve a result -> link a task
#   -> `orbit run job research_investigation` -> wait and report the result
#   -> validate/accept -> assess -> print the four dashboard panels
#
# The question is deliberately cheap and answerable by a small deterministic
# script with a control: a simulated fair coin and a 60%-heads control coin, 100,000
# flips each, seeded. A correct run finishes in minutes.
#
# Everything it writes goes to the disposable corpus (which it creates, and
# refuses to reuse if it already holds anything), the Orbit task store and an
# evidence directory; it prints the commands that remove all of it. It never
# runs `orbit plugin add|enable|upgrade` and never grants anything: the plugin
# must already be installed and enabled.
#
# Guard: it does nothing but print its plan unless it is given --live and
# ORBIT_RESEARCH_LIVE_CONFIRM=yes. `--plan` prints the plan and stops. The
# refusal paths are tested with a stub `orbit` and nothing else on PATH
# (crates/orbit-research-cli/tests/e2e_live_script.rs).
#
# usage: scripts/e2e-live.sh --corpus DIR --crew NAME --live [options]
set -eu

usage() {
    cat <<'EOF'
usage: scripts/e2e-live.sh --corpus DIR --crew NAME [options]

  --corpus DIR       Disposable corpus to create. Refused if it exists and is
                     not empty, or if it lies inside a Git work tree.
  --crew NAME        The Orbit crew that investigates (it must be defined and
                     enabled; `orbit config show` lists crews).
  --workspace NAME   Orbit workspace name to register for the corpus
                     (default: research-live-<corpus directory name>).
  --evidence DIR     Evidence directory (default: <corpus>.evidence).
  --max-minutes N    Give up waiting for the run after N minutes (default 45;
                     the run is left going and its id is printed).
  --verdict V        Assessment verdict: inconclusive (default), supports or
                     refutes. The script never infers one from the result.
  --strength S       anecdote (default), suggestive or strong.
  --no-accept        Stop after reporting the delivered result.
  --resume           Continue a half-finished run: --corpus and --evidence name
                     an existing run, and steps already recorded in its
                     state.env are skipped.
  --live             Required to run, together with ORBIT_RESEARCH_LIVE_CONFIRM=yes
                     in the environment: this registers a workspace and starts a
                     real provider, which spends money. Without both, the script
                     prints the plan and changes nothing.
  --plan             Print the steps and exit without touching anything.
  -h, --help         This text.

Requires on PATH: orbit (with the research plugin installed and enabled),
orbit-research, git and python3.
EOF
}

die() {
    printf 'e2e-live: %s\n' "$*" >&2
    exit 1
}

log() {
    printf '== %s\n' "$*"
}

corpus=""
crew=""
workspace=""
evidence=""
max_minutes=45
verdict=inconclusive
strength=anecdote
accept=1
resume=0
plan_only=0
live=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --corpus) [ "$#" -ge 2 ] || die "--corpus needs a directory"; corpus=$2; shift 2 ;;
        --crew) [ "$#" -ge 2 ] || die "--crew needs a name"; crew=$2; shift 2 ;;
        --workspace) [ "$#" -ge 2 ] || die "--workspace needs a name"; workspace=$2; shift 2 ;;
        --evidence) [ "$#" -ge 2 ] || die "--evidence needs a directory"; evidence=$2; shift 2 ;;
        --max-minutes) [ "$#" -ge 2 ] || die "--max-minutes needs a number"; max_minutes=$2; shift 2 ;;
        --verdict) [ "$#" -ge 2 ] || die "--verdict needs a value"; verdict=$2; shift 2 ;;
        --strength) [ "$#" -ge 2 ] || die "--strength needs a value"; strength=$2; shift 2 ;;
        --no-accept) accept=0; shift ;;
        --resume) resume=1; shift ;;
        --live) live=1; shift ;;
        --plan) plan_only=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) usage >&2; die "unknown argument: $1" ;;
    esac
done

case "$verdict" in inconclusive|supports|refutes) ;; *) die "--verdict must be inconclusive, supports or refutes" ;; esac
case "$strength" in anecdote|suggestive|strong) ;; *) die "--strength must be anecdote, suggestive or strong" ;; esac
case "$max_minutes" in ''|*[!0-9]*) die "--max-minutes must be a whole number" ;; esac

QUESTION="Is Python's seeded random.Random unbiased enough that a fair simulated coin lands heads within one percentage point of 50% over 100,000 flips?"
HYPOTHESIS_TITLE="A seeded fair coin stays within one point of 50 percent"
HYPOTHESIS_BODY="random.Random(42), 100,000 flips: the heads rate of the fair coin lies in [49%, 51%]. A control coin biased to 60% heads falls outside that band, which shows the check can fail."
RESULT_TITLE="Fair coin against a biased control"
OBJECTIVE="Write a small Python script under code/ that flips random.Random(42) 100,000 times for a fair coin and for a control coin with P(heads)=0.6, records both heads rates in artifacts/result.json and states whether each lies in [49%, 51%]. The control must fall outside the band; if it does not, the result is inconclusive."

print_plan() {
    cat <<EOF
Steps (nothing is executed by --plan):
  1. preflight: orbit and the research plugin (installed and enabled), the crew, orbit-research, git, python3
  2. create the disposable corpus with \`orbit-research workspace init\` and register it as an Orbit workspace
  3. capture the question:
       $QUESTION
  4. create the hypothesis and reserve the result, then draft (\`plan\`) and \`link\` its task
  5. approve the task, set its crew and run \`orbit run job research_investigation\`
  6. wait for the run (at most $max_minutes minutes) and report the delivered result and the run's provider and model
  7. validate and accept (\`accept\` re-validates; a second call must be idempotent)
  8. assess the hypothesis ($verdict, $strength; the script never infers a verdict)
  9. print the four dashboard panels
 10. write the evidence directory and print the cleanup commands
EOF
}

if [ "$plan_only" = 1 ]; then
    print_plan
    exit 0
fi

if [ "$live" != 1 ] || [ "${ORBIT_RESEARCH_LIVE_CONFIRM:-}" != yes ]; then
    print_plan
    printf '\n'
    die "dry run only: this registers a workspace and starts a real provider, which spends money. To run it, pass --live and set ORBIT_RESEARCH_LIVE_CONFIRM=yes."
fi
[ -n "$corpus" ] || { usage >&2; die "--corpus is required"; }
[ -n "$crew" ] || { usage >&2; die "--crew is required"; }

for program in orbit orbit-research git python3; do
    command -v "$program" > /dev/null 2>&1 || die "missing required program: $program"
done

# ---- paths ---------------------------------------------------------------

parent=$(dirname -- "$corpus")
mkdir -p -- "$parent"
parent=$(CDPATH= cd -- "$parent" && pwd -P)
corpus="$parent/$(basename -- "$corpus")"
[ -n "$evidence" ] || evidence="$corpus.evidence"
case "$evidence" in /*) ;; *) evidence="$(pwd -P)/$evidence" ;; esac
[ -n "$workspace" ] || workspace="research-live-$(basename -- "$corpus" | tr -c 'A-Za-z0-9\n' '-')"
state="$evidence/state.env"

if [ "$resume" = 1 ]; then
    [ -f "$state" ] || die "--resume needs $state from the earlier run"
    grep -qx "CORPUS=$corpus" "$state" || die "$state belongs to a different corpus"
    [ -d "$corpus/.git" ] || die "--resume needs the corpus at $corpus"
else
    if [ -e "$corpus" ]; then
        if [ -d "$corpus" ] && [ -z "$(ls -A -- "$corpus")" ]; then
            rmdir -- "$corpus"
        else
            die "refusing to touch $corpus: it exists and is not empty. A disposable corpus is created fresh; pass a new path (or --resume for an interrupted run)."
        fi
    fi
    [ ! -e "$evidence" ] || die "refusing to reuse the evidence directory $evidence; pass a new --evidence or --resume"
    if git -C "$parent" rev-parse --is-inside-work-tree > /dev/null 2>&1; then
        die "refusing to create the corpus inside the Git work tree that contains $parent; use a directory outside every repository"
    fi
fi
mkdir -p -- "$evidence"

# ---- evidence helpers ----------------------------------------------------

n=0
last=""
timings="$evidence/timings.tsv"
[ -f "$timings" ] || printf 'step\texit\tseconds\tcommand\n' > "$timings"
if [ -f "$evidence/.counter" ]; then n=$(cat "$evidence/.counter"); fi

state_get() {
    [ -f "$state" ] || return 0
    sed -n "s/^$1=//p" "$state" | tail -n 1
}

state_set() {
    printf '%s=%s\n' "$1" "$2" >> "$state"
}

# jget FILE EXPR: print a Python expression over the parsed JSON file `d`
# ("" for null). The expressions are this script's own constants.
jget() {
    python3 - "$1" "$2" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))
v = eval(sys.argv[2], {}, {"d": d})
print("" if v is None else v)
PY
}

# json_obj KEY=VALUE...: a JSON object of string values.
json_obj() {
    python3 -c 'import json, sys; print(json.dumps(dict(a.split("=", 1) for a in sys.argv[1:])))' "$@"
}

# link_input RESULT PLAN_FILE: the `link` input drafted by `plan`.
link_input() {
    python3 - "$1" "$2" <<'PY'
import json, sys
plan = json.load(open(sys.argv[2]))
print(json.dumps({
    "research_id": sys.argv[1],
    "request_key": "live-link",
    "title": plan["title"],
    "description": plan["description"],
    "acceptance_criteria": plan["acceptance_criteria"],
}))
PY
}

# step NAME COMMAND...: run, time and record a command. Output goes to
# $evidence/NN-NAME.out/.err; $last names the stdout file. A failure stops the
# script with the command's own diagnostics unless STEP_ALLOW_FAIL=1.
step() {
    name=$1
    shift
    n=$((n + 1))
    printf '%s' "$n" > "$evidence/.counter"
    id=$(printf '%02d-%s' "$n" "$name")
    printf '%s\t$ %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" >> "$evidence/commands.log"
    started=$(date +%s)
    status=0
    "$@" > "$evidence/$id.out" 2> "$evidence/$id.err" || status=$?
    elapsed=$(($(date +%s) - started))
    printf '%s\t%s\t%s\t%s\n' "$id" "$status" "$elapsed" "$*" >> "$timings"
    last="$evidence/$id.out"
    if [ "$status" -ne 0 ] && [ "${STEP_ALLOW_FAIL:-0}" != 1 ]; then
        printf 'e2e-live: step %s failed (exit %s): %s\n' "$id" "$status" "$*" >&2
        sed 's/^/  | /' "$evidence/$id.err" >&2 || true
        sed 's/^/  | /' "$evidence/$id.out" >&2 || true
        printf 'e2e-live: evidence so far is in %s\n' "$evidence" >&2
        exit 1
    fi
    return "$status"
}

# The operator override is Orbit's audited identity for a script with no caller.
ORBIT_OPERATOR=${ORBIT_OPERATOR:-1}
export ORBIT_OPERATOR
ORBIT_BIN=${ORBIT_BIN:-$(command -v orbit)}
export ORBIT_BIN

# ---- 1. preflight ---------------------------------------------------------

log "preflight (evidence: $evidence)"
step orbit-version orbit --version
step research-version orbit-research --version
step plugin-show orbit plugin show research
STEP_ALLOW_FAIL=1
step plugin-doctor orbit plugin doctor || log "orbit plugin doctor reported findings (see $evidence); continuing"
STEP_ALLOW_FAIL=0
[ -n "$(state_get CORPUS)" ] || { state_set CORPUS "$corpus"; state_set WORKSPACE "$workspace"; state_set CREW "$crew"; }

provider=$(orbit config get "crews.$crew.provider" 2>/dev/null || true)
case "$provider" in ''|null) die "crew '$crew' is not defined; \`orbit config show\` lists the crews" ;; esac
if [ "$(orbit config get "crews.$crew.enabled" 2>/dev/null || true)" = false ]; then
    die "crew '$crew' is disabled; enable it with \`orbit config set crews.$crew.enabled true\`"
fi
model=$(orbit config get "crews.$crew.model" 2>/dev/null || true)
log "crew $crew: provider=$provider model=${model:-unset}"

# Tools register as orbit.research.<verb> for a verified first-party install and
# as research.<verb> for any other source.
if orbit tool show orbit.research.version > /dev/null 2>&1; then
    tool_prefix=orbit.research
elif orbit tool show research.version > /dev/null 2>&1; then
    tool_prefix=research
else
    die "the research plugin's tools are not on the tool surface; install and enable the plugin first (that needs the owner's grant)"
fi
log "plugin tools: $tool_prefix.<verb>"

# ---- 2. the disposable corpus ----------------------------------------------

if [ -z "$(state_get INIT)" ]; then
    log "creating the disposable corpus $corpus"
    step corpus-init orbit-research --json workspace init "$corpus"
    cd "$corpus"
    step workspace-register orbit workspace init --name "$workspace" --ship-mode local
    if [ -z "$(git config user.email || true)" ]; then
        git config user.name "orbit-research live smoke"
        git config user.email "live-smoke@example.invalid"
    fi
    step commit-orbit-ignore git add -- .gitignore
    step commit-orbit-ignore-commit git commit -q -m "Commit Orbit's managed ignore block"
    state_set INIT done
fi
cd "$corpus"
[ -z "$(git status --porcelain)" ] || die "the corpus is not clean after setup: $(git status --porcelain)"
branch=$(git rev-parse --abbrev-ref HEAD)

# ---- 3. capture, hypothesis, reservation ----------------------------------

if [ -z "$(state_get QUESTION_ID)" ]; then
    log "capturing the question"
    step capture orbit-research --json research capture --corpus "$corpus" --text "$QUESTION" --tag e2e-live
    state_set QUESTION_ID "$(jget "$last" 'd["id"]')"
fi
question=$(state_get QUESTION_ID)

if [ -z "$(state_get HYPOTHESIS_ID)" ]; then
    log "creating the hypothesis"
    step hypothesis orbit-research --json research create --corpus "$corpus" --kind H \
        --title "$HYPOTHESIS_TITLE" --body "$HYPOTHESIS_BODY" --derived-from "$question" \
        --request-key live-hypothesis
    state_set HYPOTHESIS_ID "$(jget "$last" 'd["id"]')"
fi
hypothesis=$(state_get HYPOTHESIS_ID)

if [ -z "$(state_get RESULT_ID)" ]; then
    log "reserving the research result"
    step reserve orbit-research --json research create --corpus "$corpus" --kind R --status planned \
        --title "$RESULT_TITLE" --body "$OBJECTIVE" --derived-from "$question" \
        --derived-from "$hypothesis" --request-key live-reserve
    state_set RESULT_ID "$(jget "$last" 'd["id"]')"
fi
result=$(state_get RESULT_ID)
log "records: question $question, hypothesis $hypothesis, result $result"

# ---- 4. plan and link ------------------------------------------------------

if [ -z "$(state_get TASK_ID)" ]; then
    log "drafting and linking the Orbit task"
    plan_input=$(json_obj shape=investigation "research_id=$result" "objective=$OBJECTIVE")
    step plan orbit tool run "$tool_prefix.plan" --input "$plan_input" --full
    plan_file=$last
    link_json=$(link_input "$result" "$plan_file")
    step link orbit tool run "$tool_prefix.link" --input "$link_json" --full
    state_set TASK_ID "$(jget "$last" 'd["task_id"]')"
    # An identical retry must adopt the same task and create no second one.
    step link-retry orbit tool run "$tool_prefix.link" --input "$link_json" --full
    [ "$(jget "$last" 'd["task_id"]')" = "$(state_get TASK_ID)" ] || die "link retry returned a different task"
    [ "$(jget "$last" 'd["created"]')" = False ] || die "link retry created a second task"
fi
task=$(state_get TASK_ID)
log "task $task"

if [ -z "$(state_get APPROVED)" ]; then
    step task-crew orbit tool run orbit.task.update --input "$(json_obj "id=$task" "crew=$crew")"
    step task-approve orbit tool run orbit.task.update --input "$(json_obj "id=$task" status=backlog)"
    state_set APPROVED yes
fi

# ---- 5. the investigation ----------------------------------------------------

if [ -z "$(state_get RUN_ID)" ]; then
    log "starting orbit run job research_investigation (crew $crew)"
    step run-submit orbit run job research_investigation --workspace "$workspace" \
        --input "task=$task" --input "base_branch=$branch" --input "crew=$crew" --json
    state_set RUN_ID "$(jget "$last" 'd["run_id"]')"
    state_set RUN_STARTED "$(date +%s)"
fi
run_id=$(state_get RUN_ID)
started_at=$(state_get RUN_STARTED)
deadline=$((started_at + max_minutes * 60))

log "waiting for $run_id (at most $max_minutes minutes)"
run_state=""
while :; do
    orbit run show "$run_id" --json > "$evidence/run-show.json" 2> /dev/null || true
    run_state=$(jget "$evidence/run-show.json" 'd["run"]["state"]' 2> /dev/null || true)
    printf '   %s  %s  (%ss)\n' "$(date +%H:%M:%S)" "${run_state:-unknown}" "$(($(date +%s) - started_at))"
    case "$run_state" in
        success|failed|cancelled|canceled|timeout|timed_out|error) break ;;
    esac
    if [ "$(date +%s)" -ge "$deadline" ]; then
        printf 'e2e-live: still %s after %s minutes. The run is left going; follow it with\n  orbit run show %s\n  orbit run cancel %s\nthen rerun with --resume.\n' \
            "${run_state:-unknown}" "$max_minutes" "$run_id" "$run_id" >&2
        exit 3
    fi
    sleep 10
done
printf '%s\torbit run show %s --json (final)\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$run_id" >> "$evidence/commands.log"

# ---- 6. report ---------------------------------------------------------------

step task-show orbit tool run orbit.task.show --input "$(python3 -c 'import json, sys; print(json.dumps({"id": sys.argv[1], "fields": ["status", "job_run_id"]}))' "$task")"
task_status=$(jget "$last" 'd["status"]')
report="$evidence/summary.txt"
{
    printf 'run:        %s\n' "$run_id"
    printf 'state:      %s\n' "$run_state"
    printf 'task:       %s (%s, run %s)\n' "$task" "$task_status" "$(jget "$last" 'd["job_run_id"]')"
    printf 'crew:       %s (provider %s, configured model %s)\n' "$crew" "$provider" "${model:-unset}"
    printf 'resolved:   crew=%s model=%s\n' "$(jget "$evidence/run-show.json" 'd["run"].get("resolved_crew")')" "$(jget "$evidence/run-show.json" 'd["run"].get("crew_model")')"
    printf 'invocations (provider/model reported by Orbit):\n'
    python3 - "$evidence/run-show.json" <<'PY'
import json, sys
d = json.load(open(sys.argv[1]))["run"]
for activity in d.get("activity_provenance") or []:
    for invocation in activity.get("invocations") or []:
        print("  %s: provider=%s model=%s" % (activity.get("activity_id"), invocation.get("provider"), invocation.get("model")))
print("duration_ms: %s" % d.get("duration_ms"))
for s in d.get("steps") or []:
    print("  step %-12s %-8s %s ms%s" % (s.get("target_id"), s.get("state"), s.get("duration_ms"), "  " + str(s.get("error_message")) if s.get("error_message") else ""))
PY
} > "$report"
cat "$report"

if [ "$run_state" != success ]; then
    cat >&2 <<EOF
e2e-live: the run did not succeed ($run_state). Nothing was merged. Inspect it with
  orbit run show $run_id
  orbit run logs $run_id
The evidence is in $evidence. The task and corpus are left in place for inspection.
EOF
    exit 1
fi

step show-result orbit-research --json research show --corpus "$corpus" --id "$result"
result_file=$last
readme=$(jget "$result_file" 'd["path"]')
log "delivered $result at $readme (status $(jget "$result_file" 'd["metadata"]["status"]'), orbit $(jget "$result_file" 'd["metadata"].get("orbit")'))"
step delivery-commit git show --stat --format='%H %s' HEAD
cat "$last"
printf '\n-- Result section of %s --\n' "$readme"
awk '/^## Result/{p=1; next} /^## /{p=0} p' "$corpus/$readme" | tee "$evidence/result-section.txt"
printf -- '-- end --\n\n'
if [ -d "$corpus/$(dirname -- "$readme")/artifacts" ]; then
    cp -R "$corpus/$(dirname -- "$readme")/artifacts" "$evidence/artifacts"
fi
[ -z "$(git status --porcelain)" ] || die "the corpus is not clean after delivery: $(git status --porcelain)"

# ---- 7. validate and accept --------------------------------------------------

if [ "$accept" = 0 ]; then
    cat <<EOF
Stopping before acceptance (--no-accept). To continue:
  orbit tool run $tool_prefix.accept --input '{"task_id":"$task","research_id":"$result"}'
EOF
else
    log "accepting $result (accept re-validates the published commit)"
    accept_input=$(json_obj "task_id=$task" "research_id=$result")
    step accept orbit tool run "$tool_prefix.accept" --input "$accept_input" --full
    cat "$last"
    [ "$(jget "$last" 'd["recorded"]')" = True ] || die "accept did not record acceptance"
    step accept-again orbit tool run "$tool_prefix.accept" --input "$accept_input" --full
    [ "$(jget "$last" 'd["recorded"]')" = False ] || die "a second accept was not idempotent"
    log "acceptance stored and idempotent"

    # ---- 8. assess ---------------------------------------------------------
    log "assessing $hypothesis: $verdict ($strength)"
    step hypothesis-blob orbit-research --json research show --corpus "$corpus" --id "$hypothesis"
    blob=$(jget "$last" 'd["git_blob"]')
    revision=$(jget "$last" 'd["metadata"]["revision"]')
    step assess orbit-research --json research assess --corpus "$corpus" --id "$hypothesis" \
        --expected-blob "$blob" --research "$result" --revision "$revision" \
        --verdict "$verdict" --strength "$strength" \
        --note "live smoke run: the verdict was stated by the operator script, never inferred from the run"
    cat "$last"
fi

# ---- 9. the panels ----------------------------------------------------------

mkdir -p "$evidence/panels"
for panel in open-questions awaiting-acceptance hypotheses corpus-health; do
    step "panel-$panel" orbit tool run "$tool_prefix.$panel" --input '{}' --full
    cp "$last" "$evidence/panels/$panel.json"
    printf '\n-- panel: %s --\n' "$panel"
    python3 - "$last" <<'PY'
import json, sys
value = json.load(open(sys.argv[1]))
if isinstance(value, list):
    for row in value:
        print("  " + " | ".join("%s=%s" % (k, row[k]) for k in sorted(row)))
else:
    for key in sorted(value):
        print("  %s: %s" % (key, value[key]))
PY
done

# ---- 10. cleanup -------------------------------------------------------------

worktrees=$(git -C "$corpus" worktree list --porcelain | sed -n 's/^worktree //p' | grep '/\.orbit/state/worktrees/' || true)
{
    printf '# Cleanup for the live smoke run. Nothing below has been run.\n'
    printf '# 1. Deregister the disposable workspace from Orbit (this keeps the corpus on disk):\n'
    printf 'orbit workspace remove %s\n' "$workspace"
    printf '# 2. Remove the run worktrees (Orbit also reaps them on its worktree sweep):\n'
    if [ -n "$worktrees" ]; then
        printf '%s\n' "$worktrees" | while IFS= read -r path; do
            printf 'git -C %s worktree remove --force %s\n' "$corpus" "$path"
        done
    else
        printf '# (no run worktrees remain)\n'
    fi
    printf '# 3. Delete the corpus and, when you have read it, the evidence:\n'
    printf 'rm -rf %s\n' "$corpus"
    printf 'rm -rf %s\n' "$evidence"
    printf '# The Orbit task %s stays in the task store as history; close it with `orbit.task.update` if you want it out of the backlog views.\n' "$task"
} > "$evidence/cleanup.txt"

log "done: run $run_id $run_state; evidence in $evidence"
printf '\n'
cat "$evidence/cleanup.txt"
