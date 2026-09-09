## MODIFIED Requirements

### Requirement: Fetch attempts logged
Every fetch attempt SHALL be recorded with timestamp, duration, and error detail on failure. A fetch attempt's outcome (success or failure) SHALL be derivable from whether that entry's error detail is present, not stored as a separate field: a failed attempt SHALL always carry error detail, and a successful attempt SHALL never carry error detail.

#### Scenario: Failure recorded with cause
- **WHEN** an http source times out
- **THEN** a fetch log entry exists with error detail naming the timeout

#### Scenario: Success recorded with no error detail
- **WHEN** a fetch succeeds
- **THEN** its fetch log entry carries no error detail, distinguishing it from a failure
