-- Migration 21: Add is_default column to osm_modules.
-- Tracks which module is the built-in default, so the UI can hide the delete
-- button and so that source can be updated freely without losing this distinction.
ALTER TABLE osm_modules ADD COLUMN is_default BOOLEAN NOT NULL DEFAULT 0;
