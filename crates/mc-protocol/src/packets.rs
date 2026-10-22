//! Every protocol 47 packet, with ids from `EnumConnectionState` and field encodings from each
//! packet's `readPacketData` / `writePacketData`. Field names follow the wire meaning; raw
//! fixed-point and angle encodings are kept as on the wire (callers convert).
//!
//! Chat components travel as their JSON text; parsing them is the chat layer's job.

use crate::buf::{Error, MetaValue, Metadata, Reader, Result, Slot, Uuid, Writer};
use mc_core::nbt::Compound;
use mc_core::BlockPos;

/// Body codec for one packet (the id is handled by the state enums).
pub trait Packet: Sized {
    fn write(&self, w: &mut Writer);
    fn read(r: &mut Reader) -> Result<Self>;
}

macro_rules! rd {
    ($r:ident, varint) => { $r.varint()? };
    ($r:ident, varlong) => { $r.varlong()? };
    ($r:ident, u8) => { $r.u8()? };
    ($r:ident, i8) => { $r.i8()? };
    ($r:ident, bool) => { $r.bool()? };
    ($r:ident, i16) => { $r.i16()? };
    ($r:ident, u16) => { $r.u16()? };
    ($r:ident, i32) => { $r.i32()? };
    ($r:ident, i64) => { $r.i64()? };
    ($r:ident, f32) => { $r.f32()? };
    ($r:ident, f64) => { $r.f64()? };
    ($r:ident, string($n:expr)) => { $r.string($n)? };
    ($r:ident, chat) => { $r.chat()? };
    ($r:ident, pos) => { $r.pos()? };
    ($r:ident, uuid) => { $r.uuid()? };
    ($r:ident, slot) => { $r.slot()? };
    ($r:ident, nbt) => { $r.nbt()? };
    ($r:ident, bytes) => { $r.byte_array()? };
    ($r:ident, metadata) => { $r.metadata()? };
}

macro_rules! wr {
    ($w:ident, $v:expr, varint) => { $w.varint(*$v) };
    ($w:ident, $v:expr, varlong) => { $w.varlong(*$v) };
    ($w:ident, $v:expr, u8) => { $w.u8(*$v) };
    ($w:ident, $v:expr, i8) => { $w.i8(*$v) };
    ($w:ident, $v:expr, bool) => { $w.bool(*$v) };
    ($w:ident, $v:expr, i16) => { $w.i16(*$v) };
    ($w:ident, $v:expr, u16) => { $w.u16(*$v) };
    ($w:ident, $v:expr, i32) => { $w.i32(*$v) };
    ($w:ident, $v:expr, i64) => { $w.i64(*$v) };
    ($w:ident, $v:expr, f32) => { $w.f32(*$v) };
    ($w:ident, $v:expr, f64) => { $w.f64(*$v) };
    ($w:ident, $v:expr, string($n:expr)) => { $w.string($v) };
    ($w:ident, $v:expr, chat) => { $w.string($v) };
    ($w:ident, $v:expr, pos) => { $w.pos(*$v) };
    ($w:ident, $v:expr, uuid) => { $w.uuid(*$v) };
    ($w:ident, $v:expr, slot) => { $w.slot($v.as_ref()) };
    ($w:ident, $v:expr, nbt) => { $w.nbt($v.as_ref()) };
    ($w:ident, $v:expr, bytes) => { $w.byte_array($v) };
    ($w:ident, $v:expr, metadata) => { $w.metadata($v) };
}

/// A packet whose fields are encoded in declaration order.
macro_rules! simple {
    ($(#[$m:meta])* $name:ident { $($(#[$fm:meta])* $f:ident: $t:ty = $k:ident $(($n:expr))?),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Debug, PartialEq)]
        pub struct $name { $($(#[$fm])* pub $f: $t),* }
        impl Packet for $name {
            #[allow(unused_variables)]
            fn write(&self, w: &mut Writer) { $(wr!(w, &self.$f, $k $(($n))?);)* }
            #[allow(unused_variables)]
            fn read(r: &mut Reader) -> Result<Self> { Ok(Self { $($f: rd!(r, $k $(($n))?)),* }) }
        }
    };
}

/// A state/direction's packet set: id table, dispatch, `From` impls.
macro_rules! packets {
    ($(#[$m:meta])* $name:ident { $($id:literal => $v:ident),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Debug, PartialEq)]
        pub enum $name { $($v($v)),* }
        impl $name {
            pub fn id(&self) -> i32 { match self { $(Self::$v(_) => $id),* } }
            /// Packet id followed by the body (an uncompressed, unframed packet).
            pub fn write(&self, w: &mut Writer) {
                w.varint(self.id());
                match self { $(Self::$v(p) => p.write(w)),* }
            }
            pub fn to_bytes(&self) -> Vec<u8> {
                let mut w = Writer::new();
                self.write(&mut w);
                w.buf
            }
            /// Decodes id + body; like vanilla, leftover bytes are an error.
            pub fn read(buf: &[u8]) -> Result<Self> {
                let mut r = Reader::new(buf);
                let id = r.varint()?;
                let p = match id {
                    $($id => Self::$v(<$v>::read(&mut r)?),)*
                    _ => return Err(Error::BadPacketId(id)),
                };
                if r.remaining() > 0 {
                    return Err(Error::Trailing(r.remaining()));
                }
                Ok(p)
            }
            pub fn name(&self) -> &'static str { match self { $(Self::$v(_) => stringify!($v)),* } }
        }
        $(impl From<$v> for $name { fn from(p: $v) -> Self { Self::$v(p) } })*
    };
}

/// An enum sent with `writeEnumValue` (its ordinal as a VarInt).
macro_rules! ordinal_enum {
    ($(#[$m:meta])* $name:ident { $($v:ident),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $name { $($v),* }
        impl $name {
            const ALL: &'static [$name] = &[$($name::$v),*];
            pub fn read(r: &mut Reader) -> Result<Self> {
                let i = r.varint()?;
                Self::ALL.get(i as usize).copied().ok_or(Error::BadValue(stringify!($name), i as i64))
            }
            pub fn write(self, w: &mut Writer) { w.varint(self as i32) }
        }
    };
}

// =============================================================================================
// Handshaking

simple!(Handshake {
    protocol_version: i32 = varint,
    address: String = string(255),
    port: u16 = u16,
    /// 1 status, 2 login
    next_state: i32 = varint,
});

packets!(
    /// `HANDSHAKING`, serverbound.
    HandshakeServerbound { 0x00 => Handshake }
);

// =============================================================================================
// Status

simple!(StatusRequest {});
simple!(StatusPing { client_time: i64 = i64 });
simple!(
    /// `ServerStatusResponse` JSON.
    StatusResponse { json: String = string(32767) }
);
simple!(StatusPong { client_time: i64 = i64 });

packets!(StatusServerbound { 0x00 => StatusRequest, 0x01 => StatusPing });
packets!(StatusClientbound { 0x00 => StatusResponse, 0x01 => StatusPong });

// =============================================================================================
// Login

simple!(LoginStart { name: String = string(16) });
simple!(EncryptionResponse { shared_secret: Vec<u8> = bytes, verify_token: Vec<u8> = bytes });
simple!(LoginDisconnect { reason: String = chat });
simple!(EncryptionRequest {
    server_id: String = string(20),
    /// X.509 encoded RSA public key
    public_key: Vec<u8> = bytes,
    verify_token: Vec<u8> = bytes,
});
simple!(LoginSuccess {
    /// Dashed UUID text (vanilla writes "" for a null id).
    uuid: String = string(36),
    name: String = string(16),
});
simple!(EnableCompression { threshold: i32 = varint });

packets!(LoginServerbound { 0x00 => LoginStart, 0x01 => EncryptionResponse });
packets!(LoginClientbound {
    0x00 => LoginDisconnect,
    0x01 => EncryptionRequest,
    0x02 => LoginSuccess,
    0x03 => EnableCompression,
});

// =============================================================================================
// Play, serverbound

simple!(KeepAliveC { key: i32 = varint });
simple!(ChatMessage { message: String = string(100) });

ordinal_enum!(UseEntityAction { Interact, Attack, InteractAt });

#[derive(Clone, Debug, PartialEq)]
pub struct UseEntity {
    pub entity_id: i32,
    pub action: UseEntityAction,
    /// Only for `InteractAt`.
    pub hit: Option<(f32, f32, f32)>,
}
impl Packet for UseEntity {
    fn write(&self, w: &mut Writer) {
        w.varint(self.entity_id);
        self.action.write(w);
        if self.action == UseEntityAction::InteractAt {
            let (x, y, z) = self.hit.unwrap_or_default();
            w.f32(x);
            w.f32(y);
            w.f32(z);
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let entity_id = r.varint()?;
        let action = UseEntityAction::read(r)?;
        let hit = if action == UseEntityAction::InteractAt { Some((r.f32()?, r.f32()?, r.f32()?)) } else { None };
        Ok(Self { entity_id, action, hit })
    }
}

simple!(
    /// `C03PacketPlayer`
    PlayerGround { on_ground: bool = bool }
);
simple!(PlayerPosition { x: f64 = f64, y: f64 = f64, z: f64 = f64, on_ground: bool = bool });
simple!(PlayerLook { yaw: f32 = f32, pitch: f32 = f32, on_ground: bool = bool });
simple!(PlayerPosLook {
    x: f64 = f64,
    y: f64 = f64,
    z: f64 = f64,
    yaw: f32 = f32,
    pitch: f32 = f32,
    on_ground: bool = bool,
});

ordinal_enum!(DiggingStatus {
    StartDestroyBlock,
    AbortDestroyBlock,
    StopDestroyBlock,
    DropAllItems,
    DropItem,
    ReleaseUseItem,
});

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerDigging {
    pub status: DiggingStatus,
    pub pos: BlockPos,
    /// `EnumFacing.getIndex`
    pub face: u8,
}
impl Packet for PlayerDigging {
    fn write(&self, w: &mut Writer) {
        self.status.write(w);
        w.pos(self.pos);
        w.u8(self.face);
    }
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(Self { status: DiggingStatus::read(r)?, pos: r.pos()?, face: r.u8()? })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct BlockPlacement {
    pub pos: BlockPos,
    /// Face index, or 255 for "use item" (pos -1,-1,-1).
    pub direction: u8,
    pub item: Option<Slot>,
    /// Hit position inside the block, quantised to 1/16 on the wire.
    pub cursor: (f32, f32, f32),
}
impl Packet for BlockPlacement {
    fn write(&self, w: &mut Writer) {
        w.pos(self.pos);
        w.u8(self.direction);
        w.slot(self.item.as_ref());
        w.u8((self.cursor.0 * 16.0) as i32 as u8);
        w.u8((self.cursor.1 * 16.0) as i32 as u8);
        w.u8((self.cursor.2 * 16.0) as i32 as u8);
    }
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(Self {
            pos: r.pos()?,
            direction: r.u8()?,
            item: r.slot()?,
            cursor: (r.u8()? as f32 / 16.0, r.u8()? as f32 / 16.0, r.u8()? as f32 / 16.0),
        })
    }
}

simple!(HeldItemChangeC { slot: i16 = i16 });
simple!(
    /// `C0APacketAnimation`: swing arm (no fields).
    AnimationC {}
);

ordinal_enum!(EntityActionKind {
    StartSneaking,
    StopSneaking,
    StopSleeping,
    StartSprinting,
    StopSprinting,
    RidingJump,
    OpenInventory,
});

#[derive(Clone, Debug, PartialEq)]
pub struct EntityAction {
    pub entity_id: i32,
    pub action: EntityActionKind,
    /// Horse jump strength 0–100.
    pub aux: i32,
}
impl Packet for EntityAction {
    fn write(&self, w: &mut Writer) {
        w.varint(self.entity_id);
        self.action.write(w);
        w.varint(self.aux);
    }
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(Self { entity_id: r.varint()?, action: EntityActionKind::read(r)?, aux: r.varint()? })
    }
}

simple!(
    /// `C0CPacketInput`; `flags`: 1 jump, 2 sneak.
    SteerVehicle { strafe: f32 = f32, forward: f32 = f32, flags: u8 = u8 }
);
simple!(CloseWindowC { window_id: u8 = u8 });
simple!(ClickWindow {
    window_id: u8 = u8,
    slot: i16 = i16,
    button: i8 = i8,
    action: i16 = i16,
    mode: i8 = i8,
    item: Option<Slot> = slot,
});
simple!(ConfirmTransactionC { window_id: u8 = u8, action: i16 = i16, accepted: bool = bool });
simple!(CreativeInventoryAction { slot: i16 = i16, item: Option<Slot> = slot });
simple!(EnchantItem { window_id: u8 = u8, enchantment: u8 = u8 });

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateSignC {
    pub pos: BlockPos,
    /// Four chat component JSON strings.
    pub lines: [String; 4],
}
impl Packet for UpdateSignC {
    fn write(&self, w: &mut Writer) {
        w.pos(self.pos);
        for l in &self.lines {
            w.string(l);
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let pos = r.pos()?;
        Ok(Self { pos, lines: [r.string(384)?, r.string(384)?, r.string(384)?, r.string(384)?] })
    }
}

simple!(
    /// `flags`: 1 invulnerable, 2 flying, 4 allow flying, 8 creative.
    PlayerAbilitiesC { flags: u8 = u8, fly_speed: f32 = f32, walk_speed: f32 = f32 }
);

#[derive(Clone, Debug, PartialEq)]
pub struct TabCompleteC {
    pub text: String,
    pub looking_at: Option<BlockPos>,
}
impl Packet for TabCompleteC {
    fn write(&self, w: &mut Writer) {
        // `StringUtils.substring(message, 0, 32767)` counts UTF-16 units.
        let s: String = char::decode_utf16(self.text.encode_utf16().take(32767)).map(|c| c.unwrap_or('\u{fffd}')).collect();
        w.string(&s);
        w.bool(self.looking_at.is_some());
        if let Some(p) = self.looking_at {
            w.pos(p);
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let text = r.string(32767)?;
        let looking_at = if r.bool()? { Some(r.pos()?) } else { None };
        Ok(Self { text, looking_at })
    }
}

simple!(ClientSettings {
    locale: String = string(7),
    view_distance: i8 = i8,
    /// 0 full, 1 system, 2 hidden
    chat_mode: i8 = i8,
    chat_colors: bool = bool,
    /// Skin layer bits (cape, jacket, sleeves, pants, hat).
    skin_parts: u8 = u8,
});

ordinal_enum!(ClientStatusKind { PerformRespawn, RequestStats, OpenInventoryAchievement });

#[derive(Clone, Debug, PartialEq)]
pub struct ClientStatus {
    pub status: ClientStatusKind,
}
impl Packet for ClientStatus {
    fn write(&self, w: &mut Writer) {
        self.status.write(w);
    }
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(Self { status: ClientStatusKind::read(r)? })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PluginMessageC {
    pub channel: String,
    pub data: Vec<u8>,
}
impl Packet for PluginMessageC {
    fn write(&self, w: &mut Writer) {
        w.string(&self.channel);
        w.bytes(&self.data);
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let channel = r.string(20)?;
        if r.remaining() > 32767 {
            return Err(Error::Frame("Payload may not be larger than 32767 bytes"));
        }
        Ok(Self { channel, data: r.rest().to_vec() })
    }
}

simple!(Spectate { target: Uuid = uuid });

ordinal_enum!(ResourcePackResult { SuccessfullyLoaded, Declined, FailedDownload, Accepted });

#[derive(Clone, Debug, PartialEq)]
pub struct ResourcePackStatus {
    pub hash: String,
    pub result: ResourcePackResult,
}
impl Packet for ResourcePackStatus {
    fn write(&self, w: &mut Writer) {
        w.string(&self.hash);
        self.result.write(w);
    }
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(Self { hash: r.string(40)?, result: ResourcePackResult::read(r)? })
    }
}

packets!(
    /// `PLAY`, serverbound.
    PlayServerbound {
        0x00 => KeepAliveC,
        0x01 => ChatMessage,
        0x02 => UseEntity,
        0x03 => PlayerGround,
        0x04 => PlayerPosition,
        0x05 => PlayerLook,
        0x06 => PlayerPosLook,
        0x07 => PlayerDigging,
        0x08 => BlockPlacement,
        0x09 => HeldItemChangeC,
        0x0A => AnimationC,
        0x0B => EntityAction,
        0x0C => SteerVehicle,
        0x0D => CloseWindowC,
        0x0E => ClickWindow,
        0x0F => ConfirmTransactionC,
        0x10 => CreativeInventoryAction,
        0x11 => EnchantItem,
        0x12 => UpdateSignC,
        0x13 => PlayerAbilitiesC,
        0x14 => TabCompleteC,
        0x15 => ClientSettings,
        0x16 => ClientStatus,
        0x17 => PluginMessageC,
        0x18 => Spectate,
        0x19 => ResourcePackStatus,
    }
);

// =============================================================================================
// Play, clientbound

simple!(KeepAliveS { key: i32 = varint });

#[derive(Clone, Debug, PartialEq)]
pub struct JoinGame {
    pub entity_id: i32,
    /// `GameType` id; the hardcore bit (8) is split out.
    pub game_mode: u8,
    pub hardcore: bool,
    pub dimension: i8,
    pub difficulty: u8,
    pub max_players: u8,
    pub level_type: String,
    pub reduced_debug_info: bool,
}
impl Packet for JoinGame {
    fn write(&self, w: &mut Writer) {
        w.i32(self.entity_id);
        w.u8(self.game_mode | if self.hardcore { 8 } else { 0 });
        w.i8(self.dimension);
        w.u8(self.difficulty);
        w.u8(self.max_players);
        w.string(&self.level_type);
        w.bool(self.reduced_debug_info);
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let entity_id = r.i32()?;
        let i = r.u8()?;
        Ok(Self {
            entity_id,
            game_mode: i & !8,
            hardcore: i & 8 == 8,
            dimension: r.i8()?,
            difficulty: r.u8()?,
            max_players: r.u8()?,
            level_type: r.string(16)?,
            reduced_debug_info: r.bool()?,
        })
    }
}

simple!(
    /// `position`: 0 chat, 1 system, 2 action bar.
    ChatS { json: String = chat, position: i8 = i8 }
);
simple!(TimeUpdate { world_age: i64 = i64, time_of_day: i64 = i64 });
simple!(EntityEquipment { entity_id: i32 = varint, slot: i16 = i16, item: Option<Slot> = slot });
simple!(SpawnPosition { pos: BlockPos = pos });
simple!(UpdateHealth { health: f32 = f32, food: i32 = varint, saturation: f32 = f32 });
simple!(Respawn { dimension: i32 = i32, difficulty: u8 = u8, game_mode: u8 = u8, level_type: String = string(16) });
simple!(
    /// `flags`: relative bits X 1, Y 2, Z 4, Y_ROT 8, X_ROT 16.
    PlayerPosLookS { x: f64 = f64, y: f64 = f64, z: f64 = f64, yaw: f32 = f32, pitch: f32 = f32, flags: u8 = u8 }
);
simple!(HeldItemChangeS { slot: i8 = i8 });
simple!(UseBed { entity_id: i32 = varint, pos: BlockPos = pos });
simple!(
    /// 0 swing, 1 hurt, 2 wake up, 3 eat, 4 crit, 5 magic crit.
    AnimationS { entity_id: i32 = varint, animation: u8 = u8 }
);
simple!(SpawnPlayer {
    entity_id: i32 = varint,
    uuid: Uuid = uuid,
    /// Fixed point, ×32.
    x: i32 = i32,
    y: i32 = i32,
    z: i32 = i32,
    /// Angle, 256 steps.
    yaw: i8 = i8,
    pitch: i8 = i8,
    current_item: i16 = i16,
    metadata: Metadata = metadata,
});
simple!(CollectItem { collected_id: i32 = varint, collector_id: i32 = varint });

#[derive(Clone, Debug, PartialEq)]
pub struct SpawnObject {
    pub entity_id: i32,
    pub kind: i8,
    pub x: i32,
    pub y: i32,
    pub z: i32,
    pub pitch: i8,
    pub yaw: i8,
    /// Object data; velocity follows only when it is > 0.
    pub data: i32,
    pub velocity: (i16, i16, i16),
}
impl Packet for SpawnObject {
    fn write(&self, w: &mut Writer) {
        w.varint(self.entity_id);
        w.i8(self.kind);
        w.i32(self.x);
        w.i32(self.y);
        w.i32(self.z);
        w.i8(self.pitch);
        w.i8(self.yaw);
        w.i32(self.data);
        if self.data > 0 {
            w.i16(self.velocity.0);
            w.i16(self.velocity.1);
            w.i16(self.velocity.2);
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let (entity_id, kind, x, y, z, pitch, yaw, data) =
            (r.varint()?, r.i8()?, r.i32()?, r.i32()?, r.i32()?, r.i8()?, r.i8()?, r.i32()?);
        let velocity = if data > 0 { (r.i16()?, r.i16()?, r.i16()?) } else { (0, 0, 0) };
        Ok(Self { entity_id, kind, x, y, z, pitch, yaw, data, velocity })
    }
}

simple!(SpawnMob {
    entity_id: i32 = varint,
    kind: u8 = u8,
    x: i32 = i32,
    y: i32 = i32,
    z: i32 = i32,
    yaw: i8 = i8,
    pitch: i8 = i8,
    head_pitch: i8 = i8,
    vx: i16 = i16,
    vy: i16 = i16,
    vz: i16 = i16,
    metadata: Metadata = metadata,
});
simple!(SpawnPainting {
    entity_id: i32 = varint,
    title: String = string(13),
    pos: BlockPos = pos,
    /// Horizontal index
    direction: u8 = u8,
});
simple!(SpawnExperienceOrb { entity_id: i32 = varint, x: i32 = i32, y: i32 = i32, z: i32 = i32, count: i16 = i16 });
simple!(
    /// Velocity in 1/8000 block per tick.
    EntityVelocity { entity_id: i32 = varint, vx: i16 = i16, vy: i16 = i16, vz: i16 = i16 }
);

#[derive(Clone, Debug, PartialEq)]
pub struct DestroyEntities {
    pub ids: Vec<i32>,
}
impl Packet for DestroyEntities {
    fn write(&self, w: &mut Writer) {
        w.varint(self.ids.len() as i32);
        for &i in &self.ids {
            w.varint(i);
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let n = r.varint()?;
        let mut ids = Vec::with_capacity(n.clamp(0, 4096) as usize);
        for _ in 0..n {
            ids.push(r.varint()?);
        }
        Ok(Self { ids })
    }
}

simple!(
    /// `S14PacketEntity`
    EntityIdle { entity_id: i32 = varint }
);
simple!(EntityRelMove { entity_id: i32 = varint, dx: i8 = i8, dy: i8 = i8, dz: i8 = i8, on_ground: bool = bool });
simple!(EntityLook { entity_id: i32 = varint, yaw: i8 = i8, pitch: i8 = i8, on_ground: bool = bool });
simple!(EntityLookMove {
    entity_id: i32 = varint,
    dx: i8 = i8,
    dy: i8 = i8,
    dz: i8 = i8,
    yaw: i8 = i8,
    pitch: i8 = i8,
    on_ground: bool = bool,
});
simple!(EntityTeleport {
    entity_id: i32 = varint,
    x: i32 = i32,
    y: i32 = i32,
    z: i32 = i32,
    yaw: i8 = i8,
    pitch: i8 = i8,
    on_ground: bool = bool,
});
simple!(EntityHeadLook { entity_id: i32 = varint, yaw: i8 = i8 });
simple!(EntityStatus { entity_id: i32 = i32, status: i8 = i8 });
simple!(AttachEntity { entity_id: i32 = i32, vehicle_id: i32 = i32, leash: u8 = u8 });
simple!(EntityMetadata { entity_id: i32 = varint, metadata: Metadata = metadata });
simple!(EntityEffect {
    entity_id: i32 = varint,
    effect_id: i8 = i8,
    amplifier: i8 = i8,
    duration: i32 = varint,
    hide_particles: i8 = i8,
});
simple!(RemoveEntityEffect { entity_id: i32 = varint, effect_id: u8 = u8 });
simple!(SetExperience { bar: f32 = f32, level: i32 = varint, total: i32 = varint });

#[derive(Clone, Debug, PartialEq)]
pub struct AttributeModifier {
    pub uuid: Uuid,
    pub amount: f64,
    pub operation: i8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AttributeSnapshot {
    pub key: String,
    pub base: f64,
    pub modifiers: Vec<AttributeModifier>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EntityProperties {
    pub entity_id: i32,
    pub attributes: Vec<AttributeSnapshot>,
}
impl Packet for EntityProperties {
    fn write(&self, w: &mut Writer) {
        w.varint(self.entity_id);
        w.i32(self.attributes.len() as i32);
        for a in &self.attributes {
            w.string(&a.key);
            w.f64(a.base);
            w.varint(a.modifiers.len() as i32);
            for m in &a.modifiers {
                w.uuid(m.uuid);
                w.f64(m.amount);
                w.i8(m.operation);
            }
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let entity_id = r.varint()?;
        let n = r.i32()?;
        let mut attributes = Vec::new();
        for _ in 0..n {
            let key = r.string(64)?;
            let base = r.f64()?;
            let m = r.varint()?;
            let mut modifiers = Vec::new();
            for _ in 0..m {
                modifiers.push(AttributeModifier { uuid: r.uuid()?, amount: r.f64()?, operation: r.i8()? });
            }
            attributes.push(AttributeSnapshot { key, base, modifiers });
        }
        Ok(Self { entity_id, attributes })
    }
}

/// `S21PacketChunkData`. `data` is the section payload described in [`crate::chunk`].
#[derive(Clone, Debug, PartialEq)]
pub struct ChunkData {
    pub x: i32,
    pub z: i32,
    /// "ground-up continuous": includes biomes and clears absent sections.
    pub full: bool,
    pub bitmask: u16,
    pub data: Vec<u8>,
}
impl Packet for ChunkData {
    fn write(&self, w: &mut Writer) {
        w.i32(self.x);
        w.i32(self.z);
        w.bool(self.full);
        w.u16(self.bitmask);
        w.byte_array(&self.data);
    }
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(Self { x: r.i32()?, z: r.i32()?, full: r.bool()?, bitmask: r.u16()?, data: r.byte_array()? })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MultiBlockChange {
    pub chunk_x: i32,
    pub chunk_z: i32,
    /// `(x << 12 | z << 8 | y, Block.getStateId)`
    pub records: Vec<(u16, i32)>,
}
impl Packet for MultiBlockChange {
    fn write(&self, w: &mut Writer) {
        w.i32(self.chunk_x);
        w.i32(self.chunk_z);
        w.varint(self.records.len() as i32);
        for &(p, s) in &self.records {
            w.u16(p);
            w.varint(s);
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let (chunk_x, chunk_z) = (r.i32()?, r.i32()?);
        let n = r.varint()?;
        let mut records = Vec::new();
        for _ in 0..n {
            records.push((r.u16()?, r.varint()?));
        }
        Ok(Self { chunk_x, chunk_z, records })
    }
}

simple!(
    /// `state`: `Block.BLOCK_STATE_IDS` value (`id << 4 | meta`).
    BlockChange { pos: BlockPos = pos, state: i32 = varint }
);
simple!(BlockAction { pos: BlockPos = pos, byte1: u8 = u8, byte2: u8 = u8, block_id: i32 = varint });
simple!(BlockBreakAnim { breaker_id: i32 = varint, pos: BlockPos = pos, stage: i8 = i8 });

/// `S26PacketMapChunkBulk`: chunks are always full; each `data` length is implied by its bitmask.
#[derive(Clone, Debug, PartialEq)]
pub struct MapChunkBulk {
    pub sky_light: bool,
    /// `(x, z, bitmask, data)`
    pub chunks: Vec<(i32, i32, u16, Vec<u8>)>,
}
impl Packet for MapChunkBulk {
    fn write(&self, w: &mut Writer) {
        w.bool(self.sky_light);
        w.varint(self.chunks.len() as i32);
        for (x, z, m, _) in &self.chunks {
            w.i32(*x);
            w.i32(*z);
            w.u16(*m);
        }
        for (.., d) in &self.chunks {
            w.bytes(d);
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let sky_light = r.bool()?;
        let n = r.varint()?;
        let mut heads = Vec::new();
        for _ in 0..n {
            heads.push((r.i32()?, r.i32()?, r.u16()?));
        }
        let mut chunks = Vec::with_capacity(heads.len());
        for (x, z, m) in heads {
            let len = crate::chunk::data_size(m.count_ones() as usize, sky_light, true);
            chunks.push((x, z, m, r.take(len)?.to_vec()));
        }
        Ok(Self { sky_light, chunks })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Explosion {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub strength: f32,
    /// Offsets from `(int)x, (int)y, (int)z`.
    pub records: Vec<(i8, i8, i8)>,
    pub player_motion: (f32, f32, f32),
}
impl Packet for Explosion {
    fn write(&self, w: &mut Writer) {
        w.f32(self.x);
        w.f32(self.y);
        w.f32(self.z);
        w.f32(self.strength);
        w.i32(self.records.len() as i32);
        for &(a, b, c) in &self.records {
            w.i8(a);
            w.i8(b);
            w.i8(c);
        }
        w.f32(self.player_motion.0);
        w.f32(self.player_motion.1);
        w.f32(self.player_motion.2);
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let (x, y, z, strength) = (r.f32()?, r.f32()?, r.f32()?, r.f32()?);
        let n = r.i32()?;
        let mut records = Vec::new();
        for _ in 0..n {
            records.push((r.i8()?, r.i8()?, r.i8()?));
        }
        Ok(Self { x, y, z, strength, records, player_motion: (r.f32()?, r.f32()?, r.f32()?) })
    }
}

simple!(Effect { effect_id: i32 = i32, pos: BlockPos = pos, data: i32 = i32, global: bool = bool });
simple!(SoundEffect {
    name: String = string(256),
    /// Fixed point, ×8.
    x: i32 = i32,
    y: i32 = i32,
    z: i32 = i32,
    volume: f32 = f32,
    /// pitch × 63
    pitch: u8 = u8,
});

/// Extra VarInt arguments for a particle id (`EnumParticleTypes.getArgumentCount`).
pub fn particle_arg_count(id: i32) -> usize {
    match id {
        36 => 2,
        37 | 38 => 1,
        _ => 0,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Particles {
    pub particle_id: i32,
    pub long_distance: bool,
    pub pos: (f32, f32, f32),
    pub offset: (f32, f32, f32),
    pub speed: f32,
    pub count: i32,
    pub args: Vec<i32>,
}
impl Packet for Particles {
    fn write(&self, w: &mut Writer) {
        w.i32(self.particle_id);
        w.bool(self.long_distance);
        w.f32(self.pos.0);
        w.f32(self.pos.1);
        w.f32(self.pos.2);
        w.f32(self.offset.0);
        w.f32(self.offset.1);
        w.f32(self.offset.2);
        w.f32(self.speed);
        w.i32(self.count);
        for i in 0..particle_arg_count(self.particle_id) {
            w.varint(self.args.get(i).copied().unwrap_or(0));
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let mut particle_id = r.i32()?;
        if !(0..=41).contains(&particle_id) {
            particle_id = 35; // BARRIER
        }
        let long_distance = r.bool()?;
        let pos = (r.f32()?, r.f32()?, r.f32()?);
        let offset = (r.f32()?, r.f32()?, r.f32()?);
        let (speed, count) = (r.f32()?, r.i32()?);
        let mut args = Vec::new();
        for _ in 0..particle_arg_count(particle_id) {
            args.push(r.varint()?);
        }
        Ok(Self { particle_id, long_distance, pos, offset, speed, count, args })
    }
}

simple!(ChangeGameState { reason: u8 = u8, value: f32 = f32 });
simple!(SpawnGlobalEntity { entity_id: i32 = varint, kind: i8 = i8, x: i32 = i32, y: i32 = i32, z: i32 = i32 });

#[derive(Clone, Debug, PartialEq)]
pub struct OpenWindow {
    pub window_id: u8,
    pub kind: String,
    pub title: String,
    pub slots: u8,
    /// Only for `EntityHorse`.
    pub entity_id: i32,
}
impl Packet for OpenWindow {
    fn write(&self, w: &mut Writer) {
        w.u8(self.window_id);
        w.string(&self.kind);
        w.string(&self.title);
        w.u8(self.slots);
        if self.kind == "EntityHorse" {
            w.i32(self.entity_id);
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let window_id = r.u8()?;
        let kind = r.string(32)?;
        let title = r.chat()?;
        let slots = r.u8()?;
        let entity_id = if kind == "EntityHorse" { r.i32()? } else { 0 };
        Ok(Self { window_id, kind, title, slots, entity_id })
    }
}

simple!(CloseWindowS { window_id: u8 = u8 });
simple!(SetSlot { window_id: i8 = i8, slot: i16 = i16, item: Option<Slot> = slot });

#[derive(Clone, Debug, PartialEq)]
pub struct WindowItems {
    pub window_id: u8,
    pub items: Vec<Option<Slot>>,
}
impl Packet for WindowItems {
    fn write(&self, w: &mut Writer) {
        w.u8(self.window_id);
        w.i16(self.items.len() as i16);
        for s in &self.items {
            w.slot(s.as_ref());
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let window_id = r.u8()?;
        let n = r.i16()?;
        let mut items = Vec::new();
        for _ in 0..n {
            items.push(r.slot()?);
        }
        Ok(Self { window_id, items })
    }
}

simple!(WindowProperty { window_id: u8 = u8, property: i16 = i16, value: i16 = i16 });
simple!(ConfirmTransactionS { window_id: u8 = u8, action: i16 = i16, accepted: bool = bool });

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateSignS {
    pub pos: BlockPos,
    pub lines: [String; 4],
}
impl Packet for UpdateSignS {
    fn write(&self, w: &mut Writer) {
        w.pos(self.pos);
        for l in &self.lines {
            w.string(l);
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let pos = r.pos()?;
        Ok(Self { pos, lines: [r.chat()?, r.chat()?, r.chat()?, r.chat()?] })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MapIcon {
    /// 0–15
    pub kind: u8,
    pub x: i8,
    pub z: i8,
    /// 0–15
    pub direction: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Maps {
    pub map_id: i32,
    pub scale: i8,
    pub icons: Vec<MapIcon>,
    /// `columns == 0` means no pixel update.
    pub columns: u8,
    pub rows: u8,
    pub x: u8,
    pub z: u8,
    pub data: Vec<u8>,
}
impl Packet for Maps {
    fn write(&self, w: &mut Writer) {
        w.varint(self.map_id);
        w.i8(self.scale);
        w.varint(self.icons.len() as i32);
        for i in &self.icons {
            w.u8((i.kind & 15) << 4 | i.direction & 15);
            w.i8(i.x);
            w.i8(i.z);
        }
        w.u8(self.columns);
        if self.columns > 0 {
            w.u8(self.rows);
            w.u8(self.x);
            w.u8(self.z);
            w.byte_array(&self.data);
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let map_id = r.varint()?;
        let scale = r.i8()?;
        let n = r.varint()?;
        let mut icons = Vec::new();
        for _ in 0..n {
            let b = r.u8()?;
            icons.push(MapIcon { kind: b >> 4 & 15, x: r.i8()?, z: r.i8()?, direction: b & 15 });
        }
        let columns = r.u8()?;
        let (rows, x, z, data) = if columns > 0 { (r.u8()?, r.u8()?, r.u8()?, r.byte_array()?) } else { (0, 0, 0, Vec::new()) };
        Ok(Self { map_id, scale, icons, columns, rows, x, z, data })
    }
}

simple!(
    /// `action`: 1 spawner, 2 command block, 3 beacon, 4 skull, 5 flower pot, 6 banner.
    UpdateTileEntity { pos: BlockPos = pos, action: u8 = u8, nbt: Option<Compound> = nbt }
);
simple!(SignEditorOpen { pos: BlockPos = pos });

#[derive(Clone, Debug, PartialEq)]
pub struct Statistics {
    pub entries: Vec<(String, i32)>,
}
impl Packet for Statistics {
    fn write(&self, w: &mut Writer) {
        w.varint(self.entries.len() as i32);
        for (k, v) in &self.entries {
            w.string(k);
            w.varint(*v);
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let n = r.varint()?;
        let mut entries = Vec::new();
        for _ in 0..n {
            entries.push((r.string(32767)?, r.varint()?));
        }
        Ok(Self { entries })
    }
}

ordinal_enum!(PlayerListAction { AddPlayer, UpdateGameMode, UpdateLatency, UpdateDisplayName, RemovePlayer });

#[derive(Clone, Debug, PartialEq)]
pub struct ProfileProperty {
    pub name: String,
    pub value: String,
    pub signature: Option<String>,
}

/// One `AddPlayerData`; fields not carried by the action are left default.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerListEntry {
    pub uuid: Uuid,
    pub name: String,
    pub properties: Vec<ProfileProperty>,
    pub game_mode: i32,
    pub ping: i32,
    pub display_name: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlayerListItem {
    pub action: PlayerListAction,
    pub entries: Vec<PlayerListEntry>,
}
impl Packet for PlayerListItem {
    fn write(&self, w: &mut Writer) {
        self.action.write(w);
        w.varint(self.entries.len() as i32);
        for e in &self.entries {
            w.uuid(e.uuid);
            match self.action {
                PlayerListAction::AddPlayer => {
                    w.string(&e.name);
                    w.varint(e.properties.len() as i32);
                    for p in &e.properties {
                        w.string(&p.name);
                        w.string(&p.value);
                        w.bool(p.signature.is_some());
                        if let Some(s) = &p.signature {
                            w.string(s);
                        }
                    }
                    w.varint(e.game_mode);
                    w.varint(e.ping);
                    w.bool(e.display_name.is_some());
                    if let Some(d) = &e.display_name {
                        w.string(d);
                    }
                }
                PlayerListAction::UpdateGameMode => w.varint(e.game_mode),
                PlayerListAction::UpdateLatency => w.varint(e.ping),
                PlayerListAction::UpdateDisplayName => {
                    w.bool(e.display_name.is_some());
                    if let Some(d) = &e.display_name {
                        w.string(d);
                    }
                }
                PlayerListAction::RemovePlayer => {}
            }
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let action = PlayerListAction::read(r)?;
        let n = r.varint()?;
        let mut entries = Vec::new();
        for _ in 0..n {
            let mut e = PlayerListEntry { uuid: r.uuid()?, ..Default::default() };
            match action {
                PlayerListAction::AddPlayer => {
                    e.name = r.string(16)?;
                    let m = r.varint()?;
                    for _ in 0..m {
                        let name = r.string(32767)?;
                        let value = r.string(32767)?;
                        let signature = if r.bool()? { Some(r.string(32767)?) } else { None };
                        e.properties.push(ProfileProperty { name, value, signature });
                    }
                    e.game_mode = r.varint()?;
                    e.ping = r.varint()?;
                    e.display_name = if r.bool()? { Some(r.chat()?) } else { None };
                }
                PlayerListAction::UpdateGameMode => e.game_mode = r.varint()?,
                PlayerListAction::UpdateLatency => e.ping = r.varint()?,
                PlayerListAction::UpdateDisplayName => e.display_name = if r.bool()? { Some(r.chat()?) } else { None },
                PlayerListAction::RemovePlayer => {}
            }
            entries.push(e);
        }
        Ok(Self { action, entries })
    }
}

simple!(PlayerAbilitiesS { flags: u8 = u8, fly_speed: f32 = f32, walk_speed: f32 = f32 });

#[derive(Clone, Debug, PartialEq)]
pub struct TabCompleteS {
    pub matches: Vec<String>,
}
impl Packet for TabCompleteS {
    fn write(&self, w: &mut Writer) {
        w.varint(self.matches.len() as i32);
        for m in &self.matches {
            w.string(m);
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let n = r.varint()?;
        let mut matches = Vec::new();
        for _ in 0..n {
            matches.push(r.string(32767)?);
        }
        Ok(Self { matches })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScoreboardObjective {
    pub name: String,
    /// 0 create, 1 remove, 2 update
    pub mode: i8,
    pub display_name: String,
    /// "integer" or "hearts"
    pub render_type: String,
}
impl Packet for ScoreboardObjective {
    fn write(&self, w: &mut Writer) {
        w.string(&self.name);
        w.i8(self.mode);
        if self.mode == 0 || self.mode == 2 {
            w.string(&self.display_name);
            w.string(&self.render_type);
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let name = r.string(16)?;
        let mode = r.i8()?;
        let (display_name, render_type) =
            if mode == 0 || mode == 2 { (r.string(32)?, r.string(16)?) } else { (String::new(), String::new()) };
        Ok(Self { name, mode, display_name, render_type })
    }
}

ordinal_enum!(ScoreAction { Change, Remove });

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateScore {
    pub name: String,
    pub action: ScoreAction,
    pub objective: String,
    pub value: i32,
}
impl Packet for UpdateScore {
    fn write(&self, w: &mut Writer) {
        w.string(&self.name);
        self.action.write(w);
        w.string(&self.objective);
        if self.action != ScoreAction::Remove {
            w.varint(self.value);
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let name = r.string(40)?;
        let action = ScoreAction::read(r)?;
        let objective = r.string(16)?;
        let value = if action != ScoreAction::Remove { r.varint()? } else { 0 };
        Ok(Self { name, action, objective, value })
    }
}

simple!(
    /// `position`: 0 list, 1 sidebar, 2 below name, 3–18 team sidebars.
    DisplayScoreboard { position: i8 = i8, name: String = string(16) }
);

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Teams {
    pub name: String,
    /// 0 create, 1 remove, 2 update, 3 add players, 4 remove players
    pub mode: i8,
    pub display_name: String,
    pub prefix: String,
    pub suffix: String,
    pub friendly_flags: i8,
    pub name_tag_visibility: String,
    pub color: i8,
    pub players: Vec<String>,
}
impl Packet for Teams {
    fn write(&self, w: &mut Writer) {
        w.string(&self.name);
        w.i8(self.mode);
        if self.mode == 0 || self.mode == 2 {
            w.string(&self.display_name);
            w.string(&self.prefix);
            w.string(&self.suffix);
            w.i8(self.friendly_flags);
            w.string(&self.name_tag_visibility);
            w.i8(self.color);
        }
        if self.mode == 0 || self.mode == 3 || self.mode == 4 {
            w.varint(self.players.len() as i32);
            for p in &self.players {
                w.string(p);
            }
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let mut t = Teams { name: r.string(16)?, mode: r.i8()?, ..Default::default() };
        if t.mode == 0 || t.mode == 2 {
            t.display_name = r.string(32)?;
            t.prefix = r.string(16)?;
            t.suffix = r.string(16)?;
            t.friendly_flags = r.i8()?;
            t.name_tag_visibility = r.string(32)?;
            t.color = r.i8()?;
        }
        if t.mode == 0 || t.mode == 3 || t.mode == 4 {
            let n = r.varint()?;
            for _ in 0..n {
                t.players.push(r.string(40)?);
            }
        }
        Ok(t)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PluginMessageS {
    pub channel: String,
    pub data: Vec<u8>,
}
impl Packet for PluginMessageS {
    fn write(&self, w: &mut Writer) {
        w.string(&self.channel);
        w.bytes(&self.data);
    }
    fn read(r: &mut Reader) -> Result<Self> {
        let channel = r.string(20)?;
        if r.remaining() > 1048576 {
            return Err(Error::Frame("Payload may not be larger than 1048576 bytes"));
        }
        Ok(Self { channel, data: r.rest().to_vec() })
    }
}

simple!(Disconnect { reason: String = chat });
simple!(ServerDifficulty { difficulty: u8 = u8 });

#[derive(Clone, Debug, PartialEq)]
pub enum CombatEvent {
    EnterCombat,
    EndCombat { duration: i32, entity_id: i32 },
    EntityDied { player_id: i32, entity_id: i32, message: String },
}
impl Packet for CombatEvent {
    fn write(&self, w: &mut Writer) {
        match self {
            CombatEvent::EnterCombat => w.varint(0),
            CombatEvent::EndCombat { duration, entity_id } => {
                w.varint(1);
                w.varint(*duration);
                w.i32(*entity_id);
            }
            CombatEvent::EntityDied { player_id, entity_id, message } => {
                w.varint(2);
                w.varint(*player_id);
                w.i32(*entity_id);
                w.string(message);
            }
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(match r.varint()? {
            0 => CombatEvent::EnterCombat,
            1 => CombatEvent::EndCombat { duration: r.varint()?, entity_id: r.i32()? },
            2 => CombatEvent::EntityDied { player_id: r.varint()?, entity_id: r.i32()?, message: r.string(32767)? },
            v => return Err(Error::BadValue("CombatEvent", v as i64)),
        })
    }
}

simple!(Camera { entity_id: i32 = varint });

#[derive(Clone, Debug, PartialEq)]
pub enum WorldBorder {
    SetSize { diameter: f64 },
    LerpSize { old_diameter: f64, new_diameter: f64, speed: i64 },
    SetCenter { x: f64, z: f64 },
    Initialize {
        x: f64,
        z: f64,
        old_diameter: f64,
        new_diameter: f64,
        speed: i64,
        portal_teleport_boundary: i32,
        warning_distance: i32,
        warning_time: i32,
    },
    SetWarningTime { time: i32 },
    SetWarningBlocks { blocks: i32 },
}
impl Packet for WorldBorder {
    fn write(&self, w: &mut Writer) {
        match *self {
            WorldBorder::SetSize { diameter } => {
                w.varint(0);
                w.f64(diameter);
            }
            WorldBorder::LerpSize { old_diameter, new_diameter, speed } => {
                w.varint(1);
                w.f64(old_diameter);
                w.f64(new_diameter);
                w.varlong(speed);
            }
            WorldBorder::SetCenter { x, z } => {
                w.varint(2);
                w.f64(x);
                w.f64(z);
            }
            WorldBorder::Initialize { x, z, old_diameter, new_diameter, speed, portal_teleport_boundary, warning_distance, warning_time } => {
                w.varint(3);
                w.f64(x);
                w.f64(z);
                w.f64(old_diameter);
                w.f64(new_diameter);
                w.varlong(speed);
                w.varint(portal_teleport_boundary);
                w.varint(warning_distance);
                w.varint(warning_time);
            }
            WorldBorder::SetWarningTime { time } => {
                w.varint(4);
                w.varint(time);
            }
            WorldBorder::SetWarningBlocks { blocks } => {
                w.varint(5);
                w.varint(blocks);
            }
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(match r.varint()? {
            0 => WorldBorder::SetSize { diameter: r.f64()? },
            1 => WorldBorder::LerpSize { old_diameter: r.f64()?, new_diameter: r.f64()?, speed: r.varlong()? },
            2 => WorldBorder::SetCenter { x: r.f64()?, z: r.f64()? },
            3 => WorldBorder::Initialize {
                x: r.f64()?,
                z: r.f64()?,
                old_diameter: r.f64()?,
                new_diameter: r.f64()?,
                speed: r.varlong()?,
                portal_teleport_boundary: r.varint()?,
                warning_distance: r.varint()?,
                warning_time: r.varint()?,
            },
            4 => WorldBorder::SetWarningTime { time: r.varint()? },
            5 => WorldBorder::SetWarningBlocks { blocks: r.varint()? },
            v => return Err(Error::BadValue("WorldBorder action", v as i64)),
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Title {
    Title(String),
    Subtitle(String),
    Times { fade_in: i32, stay: i32, fade_out: i32 },
    Clear,
    Reset,
}
impl Packet for Title {
    fn write(&self, w: &mut Writer) {
        match self {
            Title::Title(s) => {
                w.varint(0);
                w.string(s);
            }
            Title::Subtitle(s) => {
                w.varint(1);
                w.string(s);
            }
            Title::Times { fade_in, stay, fade_out } => {
                w.varint(2);
                w.i32(*fade_in);
                w.i32(*stay);
                w.i32(*fade_out);
            }
            Title::Clear => w.varint(3),
            Title::Reset => w.varint(4),
        }
    }
    fn read(r: &mut Reader) -> Result<Self> {
        Ok(match r.varint()? {
            0 => Title::Title(r.chat()?),
            1 => Title::Subtitle(r.chat()?),
            2 => Title::Times { fade_in: r.i32()?, stay: r.i32()?, fade_out: r.i32()? },
            3 => Title::Clear,
            4 => Title::Reset,
            v => return Err(Error::BadValue("Title type", v as i64)),
        })
    }
}

simple!(SetCompression { threshold: i32 = varint });
simple!(PlayerListHeaderFooter { header: String = chat, footer: String = chat });
simple!(ResourcePackSend { url: String = string(32767), hash: String = string(40) });
simple!(UpdateEntityNbt { entity_id: i32 = varint, nbt: Option<Compound> = nbt });

packets!(
    /// `PLAY`, clientbound.
    PlayClientbound {
        0x00 => KeepAliveS,
        0x01 => JoinGame,
        0x02 => ChatS,
        0x03 => TimeUpdate,
        0x04 => EntityEquipment,
        0x05 => SpawnPosition,
        0x06 => UpdateHealth,
        0x07 => Respawn,
        0x08 => PlayerPosLookS,
        0x09 => HeldItemChangeS,
        0x0A => UseBed,
        0x0B => AnimationS,
        0x0C => SpawnPlayer,
        0x0D => CollectItem,
        0x0E => SpawnObject,
        0x0F => SpawnMob,
        0x10 => SpawnPainting,
        0x11 => SpawnExperienceOrb,
        0x12 => EntityVelocity,
        0x13 => DestroyEntities,
        0x14 => EntityIdle,
        0x15 => EntityRelMove,
        0x16 => EntityLook,
        0x17 => EntityLookMove,
        0x18 => EntityTeleport,
        0x19 => EntityHeadLook,
        0x1A => EntityStatus,
        0x1B => AttachEntity,
        0x1C => EntityMetadata,
        0x1D => EntityEffect,
        0x1E => RemoveEntityEffect,
        0x1F => SetExperience,
        0x20 => EntityProperties,
        0x21 => ChunkData,
        0x22 => MultiBlockChange,
        0x23 => BlockChange,
        0x24 => BlockAction,
        0x25 => BlockBreakAnim,
        0x26 => MapChunkBulk,
        0x27 => Explosion,
        0x28 => Effect,
        0x29 => SoundEffect,
        0x2A => Particles,
        0x2B => ChangeGameState,
        0x2C => SpawnGlobalEntity,
        0x2D => OpenWindow,
        0x2E => CloseWindowS,
        0x2F => SetSlot,
        0x30 => WindowItems,
        0x31 => WindowProperty,
        0x32 => ConfirmTransactionS,
        0x33 => UpdateSignS,
        0x34 => Maps,
        0x35 => UpdateTileEntity,
        0x36 => SignEditorOpen,
        0x37 => Statistics,
        0x38 => PlayerListItem,
        0x39 => PlayerAbilitiesS,
        0x3A => TabCompleteS,
        0x3B => ScoreboardObjective,
        0x3C => UpdateScore,
        0x3D => DisplayScoreboard,
        0x3E => Teams,
        0x3F => PluginMessageS,
        0x40 => Disconnect,
        0x41 => ServerDifficulty,
        0x42 => CombatEvent,
        0x43 => Camera,
        0x44 => WorldBorder,
        0x45 => Title,
        0x46 => SetCompression,
        0x47 => PlayerListHeaderFooter,
        0x48 => ResourcePackSend,
        0x49 => UpdateEntityNbt,
    }
);

impl MetaValue {
    pub fn as_byte(&self) -> Option<i8> {
        match self {
            MetaValue::Byte(b) => Some(*b),
            _ => None,
        }
    }
}
