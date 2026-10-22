fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto");
    println!("cargo:rerun-if-changed=migrations");
    tonic_prost_build::compile_protos("proto/dq.proto")?;
    Ok(())
}
