# Flaky e2e scenarios

Scenarios that failed on CI and then passed when run again alone.
CI runs a failed scenario once more (`OD_E2E_RETRIES=1`,
[development.md](../development.md#speed-and-stability)); one that passes
then does not fail the run, so it is recorded here instead. A scenario
that shows up again is a bug to fix, not to retry: find why its first run
failed (its log is `test-results/logs/<name>.try1.log` in the run's
`e2e-test-results` artifact).

CI cannot write to the repository, so add the rows by hand. The e2e job
summary lists them under "flaky", and its log prints each as a
`flaky-row:` line:

```bash
gh run view <run-id> --log | sed -n 's/.*flaky-row: //p' >> docs/qa/flaky.md
```

| Scenario | Date (UTC) | Run |
|---|---|---|
