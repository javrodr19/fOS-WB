fn main() {
    prost_build::compile_protos(&["../proto/chat.proto"], &["../proto/"]).unwrap();
}
