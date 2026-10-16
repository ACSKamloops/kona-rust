// Made by Claude
// build.rs - only does anything with `--features cpp`: builds the C++ game library into kona.exe (door 2, via
// cxx) and builds the same library as its own program, cpp/kgame_server.exe (door 3), with the same compiler.

fn main() {
    #[cfg(feature = "cpp")]
    cpp::build();
}

#[cfg(feature = "cpp")]
mod cpp {
    pub fn build() {
        for f in ["cpp/kgame_core.h", "cpp/kgame_core.cpp", "cpp/kgame_cxx.h", "cpp/kgame_cxx.cpp", "cpp/kgame_server.cpp"] {
            println!("cargo:rerun-if-changed={f}");
        }
        cxx_build::bridge("src/door_cpp.rs")
            .file("cpp/kgame_cxx.cpp")
            .file("cpp/kgame_core.cpp")
            .std("c++17")
            .opt_level(2)
            .compile("kgame");

        let out = std::env::var("OUT_DIR").expect("OUT_DIR");
        let compiler = cc::Build::new().cpp(true).opt_level(2).get_compiler();
        let mut cmd = compiler.to_command();
        if compiler.is_like_msvc() {
            cmd.args(["/nologo", "/O2", "/EHsc", "/std:c++17", "cpp/kgame_server.cpp", "cpp/kgame_core.cpp"])
                .arg(format!("/Fo{out}\\"))
                .arg("/Fecpp\\kgame_server.exe")
                .args(["/link", "ws2_32.lib"]);
        } else {
            cmd.args(["-O2", "-std=c++17", "cpp/kgame_server.cpp", "cpp/kgame_core.cpp", "-o", "cpp/kgame_server.exe", "-lws2_32"]);
        }
        let ok = cmd.status().map(|s| s.success()).unwrap_or(false);
        if !ok {
            panic!("could not build cpp/kgame_server.exe");
        }
    }
}
// Made by Claude
