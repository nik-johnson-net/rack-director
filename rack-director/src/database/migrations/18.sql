-- OSM Modules: tracks installed operating system modules
CREATE TABLE osm_modules (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    author TEXT NOT NULL,
    description TEXT NOT NULL,
    source TEXT NOT NULL CHECK(source IN ('bundled', 'uploaded')),
    storage_prefix TEXT NOT NULL,
    archive_path TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX idx_osm_modules_name ON osm_modules(name COLLATE NOCASE);

-- OSM Operating Systems: individual OS entries within a module
CREATE TABLE osm_operating_systems (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    module_id INTEGER NOT NULL,
    dir_name TEXT NOT NULL,
    name TEXT NOT NULL,
    release TEXT NOT NULL,
    config TEXT NOT NULL,
    disabled INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (module_id) REFERENCES osm_modules(id) ON DELETE CASCADE
);

CREATE INDEX idx_osm_os_module_id ON osm_operating_systems(module_id);
CREATE UNIQUE INDEX idx_osm_os_module_dir ON osm_operating_systems(module_id, dir_name);

-- OSM Uploads: tracks async upload processing state
CREATE TABLE osm_uploads (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    filename TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('uploading', 'validating', 'extracting', 'complete', 'failed')),
    error_message TEXT,
    module_id INTEGER,
    total_bytes INTEGER,
    received_bytes INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (module_id) REFERENCES osm_modules(id) ON DELETE SET NULL
);

CREATE INDEX idx_osm_uploads_status ON osm_uploads(status);
