use std::{env, fs};

use postgresql_embedded::VersionReq;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=.cargo");
    let out_dir = env::var("OUT_DIR")?;

    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=proto/services.proto");
    tonic_prost_build::compile_protos("proto/services.proto")?;

    println!("cargo:rerun-if-changed=migrations");

    let version_req = VersionReq::parse(env!("POSTGRESQL_VERSION"))?;
    let version_dest_path = format!("{}/embedded_postgres_version.rs", out_dir);
    fs::write(
        &version_dest_path,
        format!(
            "pub const EMBEDDED_POSTGRES_VERSION: &str = \"{}\";",
            version_req
        ),
    )?;

    println!("cargo:rerun-if-env-changed=POSTGRESQL_VERSION");

    Ok(())
}
