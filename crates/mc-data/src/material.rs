//! `net.minecraft.block.material.Material`. The enum and its flags are generated.

pub use crate::generated::materials::Material;

/// Flags of a `Material` constant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterialProps {
    pub liquid: bool,
    /// `isSolid`
    pub solid: bool,
    pub blocks_light: bool,
    pub blocks_movement: bool,
    pub can_burn: bool,
    /// `isReplaceable`: placing a block here replaces it (air, water, tall grass…).
    pub replaceable: bool,
    /// `isOpaque`
    pub opaque: bool,
    /// `isToolNotRequired`: drops when broken by hand.
    pub tool_not_required: bool,
    /// `getMaterialMobility`: 0 normal, 1 destroyed by pistons, 2 blocks pistons.
    pub mobility: u8,
    /// `MapColor.colorIndex`
    pub map_color: u8,
}

impl Material {
    #[inline]
    pub fn props(self) -> &'static MaterialProps {
        &crate::generated::materials::PROPS[self as usize]
    }
}
