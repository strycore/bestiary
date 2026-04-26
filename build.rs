// Force a rebuild whenever embedded app entries change. `include_dir!`
// snapshots the directory at build time and won't otherwise notice
// new or modified entries.
fn main() {
    println!("cargo:rerun-if-changed=apps");
}
