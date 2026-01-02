fn main() -> Result<(), Box<dyn std::error::Error>> {
    // println!("cargo:rustc-env=POSTGRESQL_VERSION==18.1.0");

    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=proto");
    tonic_prost_build::compile_protos("proto/broker.proto")?;

    println!("cargo:rerun-if-changed=migrations");
    // println!("cargo:rerun-if-changed=src");
    // let status = Command::new("cargo")
    //     .args(["sqlx", "prepare"])
    //     .status()
    //     .expect("Failed to run sqlx prepare");

    // if !status.success() {
    //     panic!("sqlx prepare failed");
    // }

    Ok(())
}
