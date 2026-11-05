-- Migration 26: cloud-config layers.
-- Plain ADD COLUMN / CREATE TABLE: no table rebuild, so no foreign-key concerns.
--
-- roles.cloud_config holds the role's cloud-config layer as YAML text (comments preserved).
-- settings is a key/value table for site-wide configuration; key 'cloud_config' holds the
-- global cloud-config layer. The post-migration hook moves the bundled OSMs' retired
-- `packages`/`postinstall` variables out of roles.config_template into roles.cloud_config.
ALTER TABLE roles ADD COLUMN cloud_config TEXT;

CREATE TABLE settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
