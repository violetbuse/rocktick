fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo::rerun-if-changed=Cargo.toml");
    println!("cargo::rerun-if-changed=proto");
    // tonic_prost_build::compile_protos("proto/services.proto")?;

    println!("cargo::rerun-if-changed=migrations");

    Ok(())
}
