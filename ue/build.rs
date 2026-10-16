// Made by Claude
// build.rs - writes the C++ side of the bridge (bridge.rs.h / .cc) and builds it into kona_ue.lib.
fn main() {
    println!("cargo:rerun-if-changed=src/bridge.rs");
    cxx_build::bridge("src/bridge.rs").std("c++17").opt_level(2).compile("kona_ue_bridge");
}
// Made by Claude
