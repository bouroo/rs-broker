//! Build script for rs-broker-proto
//!
//! This build script compiles the protobuf definitions using tonic-build.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let proto_files = &["../../proto/rs_broker.proto"];

    tonic_build::configure()
        .build_client(true)
        .build_server(true)
        .file_descriptor_set_path("src/descriptor.bin")
        .out_dir("src/")
        .compile(proto_files, &["../../proto/"])?;

    println!("cargo:rerun-if-changed=../../proto/rs_broker.proto");

    Ok(())
}
