//! Compiles in the target triple the binary is built for, as `BATFILES_TARGET`, so `update`
//! knows which release asset replaces it.

fn main() {
    let target = std::env::var("TARGET").expect("Cargo sets TARGET for a build script");
    println!("cargo::rustc-env=BATFILES_TARGET={target}");
    println!("cargo::rerun-if-changed=build.rs");
}
