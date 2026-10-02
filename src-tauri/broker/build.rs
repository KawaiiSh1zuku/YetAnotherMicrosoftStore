fn main() {
    println!("cargo:rerun-if-changed=broker.manifest.xml");
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        embed_resource::compile("broker.rc", embed_resource::NONE)
            .manifest_required()
            .expect("compile broker manifest");
    }
}
