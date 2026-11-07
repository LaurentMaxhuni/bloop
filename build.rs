//! Embeds the mascot icon so Explorer, the Start Menu and Task Manager show it.

fn main() {
    println!("cargo:rerun-if-changed=installer/bloop.rc");
    println!("cargo:rerun-if-changed=installer/art/bloop.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("installer/bloop.rc", embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}
