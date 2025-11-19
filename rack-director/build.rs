use std::env;

fn main() {
    // Allow overriding database path via environment variable
    let database_path = env::var("RACK_DIRECTOR_DATABASE_PATH")
        .unwrap_or_else(|_| "/opt/rack-director".to_string());
    println!(
        "cargo:rustc-env=RACK_DIRECTOR_DATABASE_PATH={}",
        database_path
    );
    println!("cargo:rerun-if-env-changed=RACK_DIRECTOR_DATABASE_PATH");

    // Allow overriding install prefix via environment variable
    let install_prefix = env::var("RACK_DIRECTOR_INSTALL_PREFIX")
        .unwrap_or_else(|_| "/opt/rack-director".to_string());
    println!(
        "cargo:rustc-env=RACK_DIRECTOR_INSTALL_PREFIX={}",
        install_prefix
    );
    println!("cargo:rerun-if-env-changed=RACK_DIRECTOR_INSTALL_PREFIX");
}
