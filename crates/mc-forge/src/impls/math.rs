//! `MathHelper`: exact ports from mc-core, so Java mods and Rust agree bit for bit.
use crate::stubs::register;
use mc_core::math;

const MATH: &str = "net/minecraft/util/MathHelper.";

pub fn register_all() {
    let m = |name: &str| format!("{MATH}{name}");
    register(&m("func_76126_a"), |c| Ok(math::sin(c.f32(0)?).into()));
    register(&m("func_76134_b"), |c| Ok(math::cos(c.f32(0)?).into()));
    register(&m("func_76129_c"), |c| Ok(math::sqrt_float(c.f32(0)?).into()));
    register(&m("func_76133_a"), |c| Ok(math::sqrt_double(c.f64(0)?).into()));
    register(&m("func_76141_d"), |c| Ok(math::floor_float(c.f32(0)?).into()));
    register(&m("func_76128_c"), |c| Ok(math::floor_double(c.f64(0)?).into()));
    register(&m("func_76124_d"), |c| Ok(math::floor_double_long(c.f64(0)?).into()));
    register(&m("func_76140_b"), |c| Ok(math::truncate_double_to_int(c.f64(0)?).into()));
    register(&m("func_76135_e"), |c| Ok(math::abs(c.f32(0)?).into()));
    register(&m("func_76130_a"), |c| Ok(math::abs_int(c.i32(0)?).into()));
    register(&m("func_76123_f"), |c| Ok(math::ceiling_float_int(c.f32(0)?).into()));
    register(&m("func_76143_f"), |c| Ok(math::ceiling_double_int(c.f64(0)?).into()));
    register(&m("func_76125_a"), |c| Ok(math::clamp_int(c.i32(0)?, c.i32(1)?, c.i32(2)?).into()));
    register(&m("func_76131_a"), |c| Ok(math::clamp_float(c.f32(0)?, c.f32(1)?, c.f32(2)?).into()));
    register(&m("func_151237_a"), |c| Ok(math::clamp_double(c.f64(0)?, c.f64(1)?, c.f64(2)?).into()));
    register(&m("func_76142_g"), |c| Ok(math::wrap_angle_to_180_float(c.f32(0)?).into()));
    register(&m("func_76138_g"), |c| Ok(math::wrap_angle_to_180_double(c.f64(0)?).into()));
    register(&m("func_76132_a"), |c| Ok(math::abs_max(c.f64(0)?, c.f64(1)?).into()));
    register(&m("func_151236_b"), |c| Ok(math::round_up_to_power_of_two(c.i32(0)?).into()));
    register(&m("func_151235_d"), |c| { let v = c.i32(0)?; Ok((v != 0 && v & (v - 1) == 0).into()) });
}
