# Workflow events

Ostinato workflows can emit and await durable events. Events are stored in the
same SQLite journal as runs, steps, trials and the audit chain, so they survive
crashes and `ostinato resume`.

## Model

| Concept | Stored as | Written by |
|---|---|---|
| Internal event | `source = internal` | a workflow step's `emit` entry |
| External event | `source = external` | `ostinato event` |
| Audit trail | `event.emitted`, `event.ingested`, `event.received`, `event.timeout`, `event.malformed` | engine/store |
| Consumer cursor | `event_cursors(run_id, consumer, event, next_seq)` | group commit |

Every event has a per-run monotonic `seq`, an event `name`, a JSON `payload`, an
optional `emitted_by` step, and a timestamp. Events are immutable once written.

## Emitting

```cue
prepare: {
    kind: "noop"
    emit: [{
        name:    "prepared"
        payload: {stage: "prepare"}
    }]
}

publish: {
    kind: "noop"
    emit: [{
        name:     "release.ready"
        external: true          // visible to external outbox consumers
        payload:  {version: "1.2.3"}
    }]
}
```

- Emits are persisted only after the step succeeds, in the **same transaction**
  as the step row, its audit events, checkpoint and state digest.
- A step that is already committed is skipped on resume, so it never emits
  twice. A group that rolls back (cancellation) emits nothing.
- `external: true` marks the event for external consumers. Both internal and
  external events are queryable through `ostinato events`; `internal: false`
  style flags do not exist.
- The engine derives `source`: workflow emits are always `internal` or
  `external` according to the step declaration. CLI injection is always
  `external`.

## Awaiting

```cue
wait: {
    kind: "await"
    needs: ["prepare"]
    await: {
        event:     "prepared"
        source:    "internal"   // internal | external | any (default: any)
        timeoutMs: 30000        // total deadline for this step
    }
}
```

A successful await produces:

```json
{
  "text": "...", "stdout": "...", "event": "prepared",
  "source": "internal", "payload": {"stage": "prepare"},
  "json": {"stage": "prepare"}, "seq": 0, "exitCode": 0, "score": 1.0
}
```

Semantics:

- **Visibility boundary.** A group commits atomically. Events emitted by a step
  in group `N` are visible from group `N+1` onward. An await may only consume
  an internal event whose emitter is in a **strictly earlier group** of the same
  materialised cycle round. The planner rejects same-group or later emitters
  with `AwaitEmitterNotEarlier`; use an external event if the producer and
  consumer must run in the same group.
- **Queue, not broadcast-to-value.** Each consumer step has a forward-only
  cursor keyed by `(run_id, step, event name)`. It consumes the oldest matching
  event with `seq >= next_seq` first. Different await steps have different
  cursors, so one event is broadcast to every interested step. Each consumer
  consumes an event at most once.
- **Crash/resume is at-least-once.** The event cursor advances in the same
  transaction as the await step. If the process dies before that commit, the
  same event is delivered again on resume. Await steps should be side-effect
  free; put side effects in a following step.
- **Timeout is total, not per attempt.** `await.timeoutMs` (or the step
  `timeoutMs`) is a single deadline. Default: `300000` ms (5 minutes). A
  timeout fails the step, records `event.timeout`, records a timed-out trial,
  and applies the normal run failure policy.
- **No retry on await.** `retry` is rejected on `kind: "await"`; the deadline
  already covers transient store checks. `exec`, `agent`, `improve`, `gateway`
  and `cycle` fields are also rejected.
- **Cancellation.** The poll loop checks the run status on every poll
  (10 ms → capped at 250 ms) and returns `cancelled` promptly.
- **Malformed payloads.** A stored event whose JSON cannot be decoded fails the
  await with `event.malformed`; it is never silently skipped.
- **Scheduling.** Ordinary work in a group runs before its await nodes. Awaits
  are bounded separately by `maxParallel`; a sleeping await does not consume a
  slot that a runnable step could use.

## Template and condition context

The latest stored event per name is available under `events.<name>`:

```cue
exec: {cmd: ["sh", "-c", "deploy {{ events.deploy.payload.version }}"]}
```

```cue
branches: {
    fast: "events.deploy.payload.version == \"1.2.3\""
    slow: true
}
default: "slow"
```

Each entry exposes `event`, `source`, `seq`, `payload`, `emittedBy`, `ts` and
`external`. Existing context keys (`params`, `steps`, `stepStatus`, `stepRound`,
`run_id`, `workflow`, `round`) are unchanged.

## External injection and outbox CLI

Inject an event into an existing running, failed or interrupted run:

```sh
ostinato event <run_id> deploy \
  --payload '{"version":"1.2.3"}' \
  --idempotency-key deploy-1.2.3 \
  --json
```

- The run must already exist. Unknown runs are rejected explicitly; a database
  is never created implicitly.
- Injection into `succeeded` or `cancelled` runs is rejected. Failed/interrupted
  runs accept events so they can be consumed after `ostinato resume`.
- `--idempotency-key` makes retries safe: the same key returns the previously
  stored event.
- The CLI always writes `source = external`.

List or stream the outbox:

```sh
ostinato events <run_id> --json
ostinato events <run_id> --name deploy --source external --after 4
ostinato events <run_id> --follow --json
```

- Without `--follow`, `--json` prints one JSON array; text mode prints one line
  per event.
- `--follow --json` prints JSON lines and exits when the run is terminal **and**
  no unread events remain. It reads events before checking run status, so a
  short-lived run still streams its final events.
- `--follow` uses a read-only SQLite connection and never takes the injection
  or migration write lock.

## Durability and schema

```sql
CREATE TABLE workflow_events (
    run_id          TEXT NOT NULL,
    seq             INTEGER NOT NULL,
    name            TEXT NOT NULL,
    source          TEXT NOT NULL,
    payload_json    TEXT NOT NULL,
    emitted_by      TEXT,
    ts              TEXT NOT NULL,
    external        INTEGER NOT NULL DEFAULT 0,
    idempotency_key TEXT,
    PRIMARY KEY (run_id, seq),
    FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE CASCADE
);
CREATE INDEX idx_workflow_events_name ON workflow_events(run_id, name, seq);
CREATE UNIQUE INDEX idx_workflow_events_idempotency
    ON workflow_events(run_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE TABLE event_cursors (
    run_id   TEXT NOT NULL,
    consumer TEXT NOT NULL,
    event    TEXT NOT NULL,
    next_seq INTEGER NOT NULL,
    PRIMARY KEY (run_id, consumer, event),
    FOREIGN KEY (run_id) REFERENCES runs(id) ON DELETE CASCADE
);
```

Sequence numbers are allocated with `MAX(seq) + 1` inside an
`IMMEDIATE` transaction. Concurrent injectors retry busy/locked transactions
and never lose an event or duplicate a sequence. Schema creation is idempotent;
opening an already-current database does not rewrite `user_version` or take a
migration write lock.

## Security limits

- Event name: `[A-Za-z0-9._:-]{1,128}`.
- Payload: at most **256 KiB** of canonical JSON and at most **64** nested
  object/array levels.
- Payloads are data only. Ostinato never deserializes them into executable code
  or templates; values reach templates only through explicit rendering.
- Limits are enforced at emit time (planner/store) and at external injection
  time (CLI/store).
- Internal events are not automatically promoted to external visibility just
  because an external process can read the database; use `external: true` to
  declare intent for outbox consumers.

## Example

See [`examples/events.cue`](../examples/events.cue).

```sh
DB=.ostinato/events.db
ostinato --db "$DB" run examples/events.cue
# In another terminal, once the run is waiting:
ostinato --db "$DB" event <run_id> approval \
  --payload '{"approvedBy":"release-bot"}'
ostinato --db "$DB" events <run_id> --follow --json
```
