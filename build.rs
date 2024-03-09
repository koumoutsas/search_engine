use std::env;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_file = "./proto/search.proto";
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    tonic_build::configure()
        .file_descriptor_set_path(out_dir.join("search_descriptor.bin"))
        .out_dir("./src")
        .compile(&[proto_file], &["proto"])?;
    Ok(())
}