-- Migration 23: Add last_polled_at column to devices.
-- Tracks the last time the agent polled in daemon mode so that the power
-- management layer can detect whether the device is already running the agent
-- (and skip issuing an OOB power kick).
ALTER TABLE devices ADD COLUMN last_polled_at DATETIME DEFAULT 0;
