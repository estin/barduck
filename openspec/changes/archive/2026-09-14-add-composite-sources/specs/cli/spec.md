## ADDED Requirements

### Requirement: Poll and fetch accept composite roots and children

`barduck poll --source <name>` and `barduck fetch --source <name>` SHALL accept a composite source's own name, forcing/fetching the whole family, or one of its children's full `<parent>::<child>` name, with the same single-command fan-out described in data-collection (spec: data-collection — Force polling a composite source or its children). `fetch --source <name>` on a composite root prints the parsed array; on a child it prints only that child's resolved entry from the same single command run.

#### Scenario: poll on a composite root's own name

- **WHEN** `barduck poll --source load` is run against a composite source named `load`
- **THEN** the command runs once, every declared child is refreshed, and the printed outcome describes the root's command/parse result

#### Scenario: poll on a child's full name

- **WHEN** `barduck poll --source load::1m` is run
- **THEN** the parent's command runs once, every declared child is refreshed, and the printed outcome describes `load::1m`'s resulting value

#### Scenario: fetch on a composite root prints the whole array

- **WHEN** `barduck fetch --source load` is run
- **THEN** the parsed array of every child's entry is printed and nothing is written to the database

#### Scenario: fetch on a child prints just that child's entry

- **WHEN** `barduck fetch --source load::1m` is run
- **THEN** only `load::1m`'s resolved entry from the parsed array is printed and nothing is written to the database
