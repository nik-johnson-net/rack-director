# Director

## Borrowing the connection

`Director` holds `&'a mut Connection` so its methods can open transactions (see `src/database/CLAUDE.md`).

- Construct it with `Director::new(&mut conn)`, or `Director::with_power_config(&mut conn, ...)` on paths
  that perform OOB power operations.
- Methods that open a transaction take `&mut self`: `start_lifecycle_transition`, `on_boot`,
  `mark_action_success`, `mark_action_failed`, `cancel_active_transition`, `cancel_active_plan`,
  `active_or_requested_plan`, and `next_boot_target` (which calls it).
- Keep each `Director` short-lived. The borrow ends at its last use, so a handler can use a
  `Director`, then pass `&conn` to other modules' stores, then build a new `Director`
  (`Director::new(&mut conn).method()` or a fresh `let director = ...`). There is deliberately no
  accessor for the connection inside a `Director`.
- Go through `Director` for device operations rather than calling `director::store` directly. Code
  that needs a `Director` takes `&mut Connection` (e.g. the DHCP `DeviceResolver` trait,
  `install_script::render_for_device`).

## Device requests

`device_requests/` turns deferred work into a plan when a registered device has **no active plan**
(`mod.rs` holds the policy and plan creation, `store.rs` the `devices` SQL, `bmc.rs` the comparison of
desired vs detected BMC config).
`Director::active_or_requested_plan` is the one entry point: the active plan, else a plan started for a
request. `next_boot_target` (iPXE) and `/cnc/poll` (idle daemon) both call it. A request that can't be
started is logged and skipped, so it never stops a device from booting or idling. The check reads one
narrow row (`lifecycle`, the request columns, `json_extract` of `bmc`/`bmc_config`), not the whole
attributes blob, because idle daemons poll every few seconds. It opens a transaction only when there is
something to start, then re-checks inside it.

| Request | Trigger | Plan |
|---------|---------|------|
| Rediscovery | `POST /ui/devices/{uuid}/rediscover` sets `devices.rediscover_requested` (400 for New/Removed) | `discover_hardware` |
| Pending BMC config | `bmc_config` differs from detected `bmc`: address source, or static IP | `configure_bmc`, `discover_hardware` |

- The rescan after `configure_bmc` is what refreshes `attributes.bmc`; the agent's `configure_bmc` doesn't
  report BMC state back. The Discover transition ends with the same rescan for this reason, so the first
  poll after Discover doesn't see stale state and configure the BMC a second time. A BMC request also
  consumes a rediscovery request.
- Provisioned devices are included. When their plan ends (success or failure), `/cnc/poll` answers
  the idle agent with `reboot_device` (no `plan_id`), so they return to local disk rather than idling
  in the agent. This is a poll answer, not a trailing `reboot_device` action, because a failed plan never
  reaches its last action. New devices get neither request (the Discover transition covers both); Removed
  devices are skipped. There is no power kick: the request waits for the next boot or poll.
  `Director::returns_to_os_when_idle` is the one place that decides "Provisioned and idle goes back to
  its OS"; `next_boot_target` (local disk) and the poll handler (`reboot_device`) both ask it.
- **Consumed at plan creation, not completion.** A failed or cancelled request plan must not start again
  on the next poll, so the request is cleared when the plan is created and `devices.last_request_plan_id`
  points at that plan. `GET /ui/devices/{uuid}/requests` returns it as `last_request_plan` (status and
  error), and the device page shows a failed one. The operator retries with **Rediscover**.
- **Loop guard:** `devices.bmc_config_attempted` stores the BMC network settings (source, ip,
  netmask, gateway; never credentials, which `get_bmc_config` generates) the last automatic plan was
  created for. The same settings aren't retried, so a config the BMC won't take doesn't divert every
  boot. It is cleared when (a) a scan reports a BMC that matches the desired config
  (`forget_satisfied_bmc_attempt`, called from `Director::update_attributes`), so later drift is
  corrected, or (b) an operator saves `bmc_config` through `PATCH /ui/devices/{uuid}/attributes`.
- `/requests` also returns `bmc_config_state` (`in_sync` / `pending` / `attempted`), computed by the
  server so the UI doesn't repeat the rule, and `active_plan`. Netmask and gateway aren't detected, so
  editing only those leaves the state `in_sync` and triggers nothing.
- **Cancelling.** Request plans have no lifecycle transition, so `Director::cancel_active_plan`
  (`POST /ui/devices/{uuid}/plan/cancel`) cancels any active plan. A transition's plan goes through
  the transition (device ends Broken). Without it a plan on a dead device blocks every later plan.
- **Platform.** A rescan re-runs platform auto-detection, except on a Provisioned device that already
  has a platform: it keeps it (its role's disk labels were resolved against it). Deprovision to re-detect.
  A rescan of an Unprovisioned/Broken device still overwrites a manually assigned platform.

## Plan progress

Agent reports (`mark_action_success` / `mark_action_failed`) take a `ReportedStep { plan_id, action }`.
A field that is set must match the active plan and its current action, or the report is ignored as
stale. `ReportedStep::plan(id)` is the agent's plan-only form, and `ReportedStep::default()` checks
nothing, like an old agent.

`plan_progress.rs` holds plan advancement, transition cancellation and orphaned-transition repair. A plan
and its lifecycle transition must change together, so each of these, and `start_lifecycle_transition`,
runs its reads and writes in a single `in_transaction` call. `start_lifecycle_transition` validates the
current lifecycle inside that transaction, so the transition's `from_state` can't go stale.

`on_boot` and agent reports run on every iPXE hit, so they check without the lock first and only open the
transaction when the plan will change. Inside it they re-read the plan and re-apply the update to that fresh
row, so the update closure runs twice and must be idempotent (`FnMut`); log after the call, not inside it.
`repair_orphaned_transition` follows the same pattern.

Cancelling a transition cancels only the transition's own plan (by `plan_id`), never another active plan
on the device. If that plan had already finished (an orphaned transition), the transition is closed with
the plan's outcome — a successful plan still moves the device to its target state.
