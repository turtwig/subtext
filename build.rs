fn main() {
    // glibc 2.43 is the default version of acosf and atan2f. The host is
    // older, so the final link wraps those two symbols and glibc_compat.rs
    // jumps to the GLIBC_2.2.5 versions. Defining acosf itself makes the
    // dynamic loader bind the wrap back onto this binary.
    let target = std::env::var("TARGET").unwrap_or_default();
    if target == "x86_64-unknown-linux-gnu" {
        println!("cargo:rustc-link-arg=-Wl,--wrap=acosf");
        println!("cargo:rustc-link-arg=-Wl,--wrap=atan2f");
        println!("cargo:rerun-if-changed=build.rs");
    }
}
