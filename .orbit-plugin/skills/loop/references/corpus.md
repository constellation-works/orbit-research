# Creating and preparing a corpus

## A new corpus

`orbit-research workspace init <dir>` needs a new or empty directory. It makes a Git
repository on branch `main` whatever Git's `init.defaultBranch` says, with the owner
schema and the shared request storage `link` needs. It refuses a directory that holds
other files and no corpus.

Run on an existing corpus, `workspace init` writes nothing: it validates the corpus and
reports `created: false`.

## As an Orbit workspace

The plugin reads the workspace root as the corpus. Give each corpus its own directory
and have its owner register that directory as an Orbit workspace and enable the plugin
there. The plugin's `list`, `show` and `check` tools read the same records as the
`orbit-research research` subcommands of the same names.

The `research_investigation` job's `base_branch` input defaults to `main`, the branch
`workspace init` creates. For a corpus on another branch, add
`--input base_branch=<branch>` to `orbit run job`. Run the job from the corpus checkout,
or add `--workspace <name>`. Orbit owns dispatch, status and cancellation
(`orbit run show`, `orbit run cancel`).

## Other files in record directories

A record directory (`questions/`, `hypotheses/`, `theories/`, `research/`) may hold
other files, such as an index `README.md` or a `HOWTO.md`: a name that does not start
with the kind letter followed by a digit is not a record and is ignored. A name that
does, such as `R1-foo` or `Q01-why.md`, is taken as a record and must be a valid record
name (`Q001-short-slug.md`, or `R001-short-slug/` for a result directory), or every
read of the corpus reports it.

## An older corpus

A corpus made by `workspace init` is ready. For an older one, `link` refuses before
creating any task until its owner prepares shared request storage once, on the primary
checkout, with `corpus` set to its absolute path:

```sh
orbit-research workspace prepare-operations "$corpus"
```

It leaves records and Git history unchanged, and repeating it reports `changed: false`.
