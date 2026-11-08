-- Migration 27: deferred device requests.
-- Plain ADD COLUMN: no table rebuild, so no foreign-key concerns.
--
-- rediscover_requested: an operator asked for a hardware rescan; it becomes a discover_hardware
-- plan the next time the device boots or polls with no active plan.
-- bmc_config_attempted: JSON of the BMC network settings (source, ip, netmask, gateway) the last
-- automatic configure_bmc plan was created for, so a config the BMC won't take is tried once
-- rather than on every boot.
ALTER TABLE devices ADD COLUMN rediscover_requested INTEGER NOT NULL DEFAULT 0;
ALTER TABLE devices ADD COLUMN bmc_config_attempted TEXT;
-- last_request_plan_id: the plan most recently started for a request. The request itself is
-- consumed when that plan is created (a failed plan must not retry on every poll), so this is how
-- an operator sees whether it succeeded.
ALTER TABLE devices ADD COLUMN last_request_plan_id INTEGER;
