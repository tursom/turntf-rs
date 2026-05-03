fn main() {
    for proto in ["proto/client.proto", "proto/relay.proto"] {
        println!("cargo:rerun-if-changed={proto}");
    }

    let protoc = protoc_bin_vendored::protoc_bin_path().expect("vendored protoc");
    std::env::set_var("PROTOC", protoc);

    let mut client_config = prost_build::Config::new();
    client_config.bytes(["."]);
    client_config
        .compile_protos(&["proto/client.proto"], &["proto"])
        .expect("compile client proto");

    let mut relay_config = prost_build::Config::new();
    relay_config.bytes(["."]);
    relay_config.extern_path(".notifier.client.v1", "crate::proto");
    relay_config
        .compile_protos(&["proto/relay.proto"], &["proto"])
        .expect("compile relay proto");
}
