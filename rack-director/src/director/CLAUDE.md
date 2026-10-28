# Director

## Borrowing the connection

`Director` holds `&'a mut Connection` so its methods can open transactions (see `src/database/CLAUDE.md`).

- Construct it with `Director::new(&mut conn)`, or `Director::with_power_config(&mut conn, ...)` on paths
  that perform OOB power operations.
- Methods that open a transaction take `&mut self`: `start_lifecycle_transition`, `on_boot`,
  `mark_action_success`, `mark_action_failed`, `cancel_active_transition`.
- Keep each `Director` short-lived. The borrow ends at its last use, so a handler can use a
  `Director`, then pass `&conn` to other modules' stores, then build a new `Director`
  (`Director::new(&mut conn).method()` or a fresh `let director = ...`). There is deliberately no
  accessor for the connection inside a `Director`.
- Go through `Director` for device operations rather than calling `director::store` directly. Code
  that needs a `Director` takes `&mut Connection` (e.g. the DHCP `DeviceResolver` trait,
  `install_script::render_for_device`).

## Plan progress

`plan_progress.rs` holds plan advancement, transition cancellation and orphaned-transition repair. A plan
and its lifecycle transition must change together, so each of these, and `start_lifecycle_transition`,
runs its reads and writes in a single `in_transaction` call. `start_lifecycle_transition` validates the
current lifecycle inside that transaction, so the transition's `from_state` can't go stale.

`on_boot` and agent reports run on every iPXE hit, so they check without the lock first and only open the
transaction when the plan will change. Inside it they re-read the plan and re-apply the update if it moved
in between, so the update closure must be safe to call twice (`FnMut`). `repair_orphaned_transition`
follows the same pattern.

Cancelling a transition cancels only the transition's own plan (by `plan_id`), never another active plan
on the device. If that plan had already finished (an orphaned transition), the transition is closed with
the plan's outcome — a successful plan still moves the device to its target state.
