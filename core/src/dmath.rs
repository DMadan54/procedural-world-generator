//! Float functions that give the same bits on every platform.
//!
//! `f32::exp`, `powf`, `sin` and friends call the platform's C maths
//! library, and those differ in the last bit between Windows, Linux, macOS
//! and wasm. One flipped bit in a growth rate is enough to change a famine
//! two centuries later, so generation code uses these instead: they come
//! from `libm`, a pure-Rust port of musl's maths library that computes the
//! same result everywhere. (`sqrt`, `+ - * /`, `floor` and `abs` are exact
//! by IEEE 754 and need no help.)

#[inline]
pub fn exp(x: f32) -> f32 {
    libm::expf(x)
}

#[inline]
pub fn powf(x: f32, y: f32) -> f32 {
    libm::powf(x, y)
}

#[inline]
pub fn tanh(x: f32) -> f32 {
    libm::tanhf(x)
}

#[inline]
pub fn sin(x: f32) -> f32 {
    libm::sinf(x)
}

#[inline]
pub fn cos(x: f32) -> f32 {
    libm::cosf(x)
}

#[inline]
pub fn log2(x: f32) -> f32 {
    libm::log2f(x)
}

/// `x²` as one multiplication (`powi` may be lowered differently by
/// target).
#[inline]
pub fn sq(x: f32) -> f32 {
    x * x
}
