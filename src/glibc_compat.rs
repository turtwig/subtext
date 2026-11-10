//! Tail-calls for the pre-2.43 `acosf` and `atan2f`.
//!
//! `build.rs` passes `--wrap` so every other reference uses these symbols.
//! They must not be named `acosf` or `atan2f`: a definition with that name
//! is what the loader binds the versioned lookup to, and the jump never
//! reaches libm.

#[cfg(all(target_os = "linux", target_env = "gnu", target_arch = "x86_64"))]
use std::arch::global_asm;

#[cfg(all(target_os = "linux", target_env = "gnu", target_arch = "x86_64"))]
global_asm!(
    r#"
    .text
    .globl __wrap_acosf
    .type __wrap_acosf, @function
    .symver __real_acosf, acosf@GLIBC_2.2.5
    __wrap_acosf:
        jmp __real_acosf@PLT
    .size __wrap_acosf, .-__wrap_acosf

    .globl __wrap_atan2f
    .type __wrap_atan2f, @function
    .symver __real_atan2f, atan2f@GLIBC_2.2.5
    __wrap_atan2f:
        jmp __real_atan2f@PLT
    .size __wrap_atan2f, .-__wrap_atan2f
    "#
);
