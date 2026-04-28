fn main() {
    let proto = "proto/client.proto";

    println!("cargo:rerun-if-changed={proto}");

    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc");
    std::env::set_var("PROTOC", protoc);

    let mut config = prost_build::Config::new();
    config.bytes(["."]);
    config
        .compile_protos(&[proto], &["proto"])
        .expect("compile proto");
}
