// Made by Claude
// build.rs - only does anything with `--features cpp`: builds the C++ game library into kona.exe (door 2, via
// cxx) and builds the same library as its own program, cpp/kgame_server.exe (door 3), with the same compiler.

fn main() {
    // kona.exe gets a 64 MB main-thread stack reserve on Windows (the default is 1 MB; the
    // evaluator's recursion limit, eval.rs MAX_DEPTH, needs about 8). Linux and macOS give the
    // main thread 8 MB already. The reserve is virtual; pages are committed as they are touched.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" {
        if target_env == "msvc" {
            println!("cargo:rustc-link-arg-bins=/STACK:67108864");
        } else {
            println!("cargo:rustc-link-arg-bins=-Wl,--stack,67108864");
        }
    }
    #[cfg(feature = "cpp")]
    cpp::build(target_os == "windows");
}

#[cfg(feature = "cpp")]
mod cpp {
    pub fn build(windows: bool) {
        for f in ["cpp/kgame_core.h", "cpp/kgame_core.cpp", "cpp/kgame_cxx.h", "cpp/kgame_cxx.cpp", "cpp/kgame_server.cpp"] {
            println!("cargo:rerun-if-changed={f}");
        }
        cxx_build::bridge("src/door_cpp.rs")
            .file("cpp/kgame_cxx.cpp")
            .file("cpp/kgame_core.cpp")
            .std("c++17")
            .opt_level(2)
            .compile("kgame");

        // door 2 (the cxx bridge above) builds anywhere; door 3's server is a winsock program
        if !windows {
            println!("cargo:warning=cpp/kgame_server.exe (door 3) is Windows-only and was not built; door 2 is in");
            return;
        }

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
