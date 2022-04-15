fn main() {
    println!("cargo:rerun-if-changed=./web/dist/index.html");
    nodejs_bundler_codegen::Builder {
        current_dir: Some("web".into()),
        src_dir: "src".into(),
        ..Default::default()
    }
    .build();
}
