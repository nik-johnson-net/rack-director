use std::env;
use std::path::PathBuf;

#[cfg(unix)]
const DEFAULT_DATABASE_PATH: &str = "/var/lib/rack-director";
#[cfg(windows)]
const DEFAULT_DATABASE_PATH: &str = "C:\\Program Files\\Rack Director";

#[cfg(unix)]
const DEFAULT_INSTALL_PATH: &str = "/opt/rack-director";
#[cfg(windows)]
const DEFAULT_INSTALL_PATH: &str = "C:\\Program Files\\Rack Director";

fn overridable_env<T: Into<String>>(env: &str, default: T) -> String {
    let value = env::var(env).unwrap_or(default.into());
    println!("cargo:rustc-env={}={}", env, value);
    println!("cargo:rerun-if-env-changed={}", env);
    value
}

fn main() {
    // Allow overriding database path via environment variable
    let database_env = overridable_env(
        "RACK_DIRECTOR_DATABASE_PATH",
        DEFAULT_DATABASE_PATH.to_string(),
    );
    let database_path = PathBuf::from(database_env);
    overridable_env(
        "RACK_DIRECTOR_LOCAL_IMAGES_PATH",
        database_path.join("images").to_string_lossy(),
    );

    // Allow overriding install prefix via environment variable
    let install_prefix_env = overridable_env(
        "RACK_DIRECTOR_INSTALL_PREFIX",
        DEFAULT_INSTALL_PATH.to_string(),
    );
    let install_path = PathBuf::from(install_prefix_env);
    overridable_env(
        "RACK_DIRECTOR_AGENT_IMAGES_PATH",
        install_path.join("agent").to_string_lossy(),
    );
    overridable_env(
        "RACK_DIRECTOR_FIRMWARE_PATH",
        install_path.join("firmware").to_string_lossy(),
    );
    overridable_env(
        "RACK_DIRECTOR_BUNDLED_OSM_PATH",
        install_path.join("osm/default").to_string_lossy(),
    );
    overridable_env(
        "RACK_DIRECTOR_UI_PATH",
        install_path.join("ui").to_string_lossy(),
    );
}
