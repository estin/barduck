# Spec Delta

## ADDED Requirements

### Requirement: Panels-updated event for user scripts
After each panel-grid shard re-render — whether triggered by a stream-driven refresh event or the fallback timer — the page SHALL dispatch a `barduck:panels-updated` `CustomEvent` on `document`. The event's `detail` SHALL carry the names of the sources rendered in that refresh, so a listener can tell which panels changed without querying anything else first. The event SHALL fire on both the dashboard and per-source log views. Listeners subscribed once (e.g. from an end-of-body user script) SHALL observe every subsequent refresh without re-subscribing. Pages with no user scripts configured SHALL still dispatch the event.

#### Scenario: Stored value triggers the event
- **WHEN** a new value is stored for a shown source and the panels re-render
- **THEN** the page dispatches one `barduck:panels-updated` event whose detail names that source

#### Scenario: Listener survives refreshes
- **WHEN** a user script subscribes to `barduck:panels-updated` on page load and two refreshes occur
- **THEN** the listener observes both events without re-subscribing

#### Scenario: Fallback timer also fires the event
- **WHEN** no stream event arrives within the configured interval and the fallback timer re-renders the panels
- **THEN** the page dispatches `barduck:panels-updated` for that re-render
