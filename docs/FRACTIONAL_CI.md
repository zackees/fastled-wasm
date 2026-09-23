# Fractional CI and release gate

Ordinary pull requests and `main` updates run the Linux x86 lint gate. The
literal `ci-test` PR label adds Linux x86 unit and integration tests. The
`ci-full` label runs every declared platform workflow. Adding or removing a
label retriggers the workflows; `ci-full` includes the `ci-test` cells.
The `ci-test:windows-arm` label runs only Windows ARM unit and integration
tests for a targeted diagnosis. It does not satisfy the full-coverage gate.

The versioned platform manifest is [`ci/full_coverage.json`](../ci/full_coverage.json).
[`full-coverage.yml`](../.github/workflows/full-coverage.yml) polls the real
GitHub runs for all 29 workflows on one SHA and publishes a JSON report. Each
of the six release platforms must also show successful execution of the
declared unit and integration test steps. macOS ARM additionally requires its
cross-built Rust test step. A build-only success cannot satisfy release
coverage. A missing, failed, cancelled, or skipped required job or test step
fails the report. This is the
aggregate proof for the CLI platform matrix; local YAML or unit tests do not
replace it.

For a release candidate, dispatch each platform workflow at a branch whose
head is the candidate SHA, supplying its required `candidate_sha` input. The
jobs refuse a different SHA. Then dispatch `full-coverage.yml` with that SHA
and wait for its report to pass. `auto-release.yml` is manual-only and checks
the successful GitHub full-coverage report before any build or tag. To exercise
its six release builds and artifact preflight without tagging or publishing,
run `gh workflow run auto-release.yml --ref <candidate-branch> -f
candidate_sha=<40-hex-sha> -f dry_run=true`. The branch tip must be that SHA.
The preflight rejects missing/corrupt wheels, missing binaries, and wheels at
or above 100 MB; it cannot reserve a PyPI upload or verify the project's
remaining storage quota. A conflicting tag is rejected before the six builds.
The VS Code tag-push publication workflow checks the same report before
building or publishing.

The normalized issue-driven release front door and automated dispatch of the
29 full workflows are still pending. A candidate must currently be the tip of
the dispatched branch; the workflows reject a different SHA rather than
substituting the branch head. Do not create a release tag from a version bump
or invoke a direct publisher without the full-coverage report. Live PR-label
and release-candidate runs, their URLs, and runner-minute measurements are
required before claiming rollout completion.

Windows ARM previously had only a cross-build on `windows-2025`. Full CI now
includes native unit and integration jobs on `windows-11-arm`, which is already
used by the VS Code extension matrix. These new jobs have not yet passed a
real GitHub run. Until both execute and pass on the candidate SHA, the
coverage report fails and release tagging is blocked. The same real-run
requirement applies to every platform test step.

The Python wheel is tagged `py3-none-<platform>` by `setup.py`; it bundles a
native CLI executable but has no CPython extension ABI. Platform/architecture
execution is therefore the release ABI boundary here. The current test entry
points install Python 3.11.9 and do not prove the full `requires-python >=3.10`
interpreter range; that compatibility check remains a separate release gap.
