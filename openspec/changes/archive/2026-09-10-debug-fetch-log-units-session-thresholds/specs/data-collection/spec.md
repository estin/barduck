## MODIFIED Requirements

### Requirement: Stream collection
The collector SHALL run each stream source's command as a long-lived process and ingest its stdout line by line while the process lives: every well-formed `jsonl` row (spec: source-configuration — JSONL row schema) produces one reading stamped with the row's `ts` (or arrival time) and applies the row's `threshold` override when present. Threshold overrides live in daemon memory only: they are forgotten when the daemon stops, and a restarted daemon colors with config-declared bands until a new row overrides them. When the process ends for any reason (exit, signal, spawn failure), the collector SHALL record the outcome in the fetch log and reopen the command after the source's `retry_interval`; a spawn failure or immediate exit counts as a failed attempt. Shutdown stops reopening after the current wait, mirroring interval sources.

#### Scenario: Lines ingested continuously
- **WHEN** a stream command prints one `jsonl` row every second for a minute
- **THEN** roughly 60 readings are recorded without any schedule tick firing

#### Scenario: Exited stream reopens on retry_interval
- **WHEN** a stream command exits after printing one row and the source declares `retry_interval = "10s"`
- **THEN** a fetch log entry records the exit and the command is reopened roughly 10 seconds later

#### Scenario: Failing stream command retries
- **WHEN** a stream command exits non-zero immediately on every start
- **THEN** each restart is spaced by `retry_interval` and each exit is logged as failed, without affecting other sources

#### Scenario: Override forgotten on restart
- **WHEN** the daemon restarts after a stream row overrode a source's bands
- **THEN** no fetch-log or reading replay restores the override; the source uses config bands until a new row arrives
