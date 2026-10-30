//! The `GameSettings` subset the client uses, saved in vanilla's `options.txt` format.

use std::path::{Path, PathBuf};

/// `GameSettings.Options` that the UI can edit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opt {
    Fov,
    Sensitivity,
    Gamma,
    RenderDistance,
    FramerateLimit,
    GuiScale,
    AmbientOcclusion,
    Graphics,
    ViewBobbing,
    Vsync,
    Anaglyph,
    RenderClouds,
    Particles,
    Fullscreen,
    MipmapLevels,
    BlockAlternatives,
    Vbo,
    EntityShadows,
    InvertMouse,
    ChatVisibility,
    ChatColors,
    ChatLinks,
    ChatOpacity,
    ChatLinksPrompt,
    ChatScale,
    ChatWidth,
    ChatHeightFocused,
    ChatHeightUnfocused,
    /// An `EnumPlayerModelParts` toggle, by index into [`MODEL_PARTS`].
    ModelPart(u8),
    /// A `SoundCategory` volume, by index into [`SOUND_CATEGORIES`].
    SoundVolume(u8),
}

/// `EnumPlayerModelParts` names; bit `i` of the skin-parts mask.
pub const MODEL_PARTS: [&str; 7] = ["cape", "jacket", "left_sleeve", "right_sleeve", "left_pants_leg", "right_pants_leg", "hat"];

/// `SoundCategory.values()` names, in order.
pub const SOUND_CATEGORIES: [&str; 9] = ["master", "music", "record", "weather", "block", "hostile", "neutral", "player", "ambient"];

impl Opt {
    pub fn key(self) -> &'static str {
        match self {
            Opt::Fov => "options.fov",
            Opt::Sensitivity => "options.sensitivity",
            Opt::Gamma => "options.gamma",
            Opt::RenderDistance => "options.renderDistance",
            Opt::FramerateLimit => "options.framerateLimit",
            Opt::GuiScale => "options.guiScale",
            Opt::AmbientOcclusion => "options.ao",
            Opt::Graphics => "options.graphics",
            Opt::ViewBobbing => "options.viewBobbing",
            Opt::Vsync => "options.vsync",
            Opt::Anaglyph => "options.anaglyph",
            Opt::RenderClouds => "options.renderClouds",
            Opt::Particles => "options.particles",
            Opt::Fullscreen => "options.fullscreen",
            Opt::MipmapLevels => "options.mipmapLevels",
            Opt::BlockAlternatives => "options.blockAlternatives",
            Opt::Vbo => "options.vbo",
            Opt::EntityShadows => "options.entityShadows",
            Opt::InvertMouse => "options.invertMouse",
            Opt::ChatVisibility => "options.chat.visibility",
            Opt::ChatColors => "options.chat.color",
            Opt::ChatLinks => "options.chat.links",
            Opt::ChatOpacity => "options.chat.opacity",
            Opt::ChatLinksPrompt => "options.chat.links.prompt",
            Opt::ChatScale => "options.chat.scale",
            Opt::ChatWidth => "options.chat.width",
            Opt::ChatHeightFocused => "options.chat.height.focused",
            Opt::ChatHeightUnfocused => "options.chat.height.unfocused",
            Opt::ModelPart(i) => ["options.modelPart.cape", "options.modelPart.jacket", "options.modelPart.left_sleeve", "options.modelPart.right_sleeve", "options.modelPart.left_pants_leg", "options.modelPart.right_pants_leg", "options.modelPart.hat"][i as usize],
            Opt::SoundVolume(i) => ["soundCategory.master", "soundCategory.music", "soundCategory.record", "soundCategory.weather", "soundCategory.block", "soundCategory.hostile", "soundCategory.neutral", "soundCategory.player", "soundCategory.ambient"][i as usize],
        }
    }

    /// `(min, max, step)` for slider options.
    pub fn range(self) -> Option<(f32, f32, f32)> {
        Some(match self {
            Opt::Fov => (30.0, 110.0, 1.0),
            Opt::Sensitivity | Opt::Gamma | Opt::SoundVolume(_) => (0.0, 1.0, 0.0),
            Opt::ChatOpacity | Opt::ChatScale | Opt::ChatWidth | Opt::ChatHeightFocused | Opt::ChatHeightUnfocused => (0.0, 1.0, 0.0),
            Opt::RenderDistance => (2.0, 16.0, 1.0),
            Opt::FramerateLimit => (10.0, 260.0, 10.0),
            Opt::MipmapLevels => (0.0, 4.0, 1.0),
            _ => return None,
        })
    }

    /// `Options.normalizeValue`
    pub fn normalize(self, v: f32) -> f32 {
        let (min, max, step) = self.range().unwrap();
        ((snap(v, step).clamp(min, max) - min) / (max - min)).clamp(0.0, 1.0)
    }

    /// `Options.denormalizeValue`
    pub fn denormalize(self, f: f32) -> f32 {
        let (min, max, step) = self.range().unwrap();
        snap(min + (max - min) * f.clamp(0.0, 1.0), step).clamp(min, max)
    }
}

/// `gson.fromJson(value, List<String>)`; anything unparsable is an empty list.
fn parse_list(v: &str) -> Vec<String> {
    serde_json::from_str(v).unwrap_or_default()
}

/// `gson.toJson(List<String>)`: compact, with Gson's HTML-safe escapes.
fn gson_list(l: &[String]) -> String {
    let s = serde_json::to_string(l).unwrap_or_else(|_| "[]".into());
    s.replace('<', "\\u003c").replace('>', "\\u003e").replace('&', "\\u0026").replace('=', "\\u003d").replace('\'', "\\u0027")
}

/// `Options.snapToStep`
fn snap(v: f32, step: f32) -> f32 {
    if step > 0.0 { step * (v / step).round() } else { v }
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub fov: f32,
    pub mouse_sensitivity: f32,
    pub gamma: f32,
    pub render_distance: i32,
    pub limit_framerate: i32,
    pub gui_scale: i32,
    pub ambient_occlusion: i32,
    pub fancy_graphics: bool,
    pub view_bobbing: bool,
    pub vsync: bool,
    pub anaglyph: bool,
    /// `clouds`: 0 off, 1 fast, 2 fancy.
    pub clouds: i32,
    /// `particleSetting`: 0 all, 1 decreased, 2 minimal.
    pub particles: i32,
    pub fullscreen: bool,
    pub mipmap_levels: i32,
    pub block_alternatives: bool,
    pub use_vbo: bool,
    pub entity_shadows: bool,
    pub invert_mouse: bool,
    /// `EnumChatVisibility`: 0 full, 1 commands only, 2 hidden.
    pub chat_visibility: i32,
    pub chat_colors: bool,
    pub chat_links: bool,
    pub chat_links_prompt: bool,
    pub chat_opacity: f32,
    pub chat_scale: f32,
    pub chat_width: f32,
    pub chat_height_focused: f32,
    pub chat_height_unfocused: f32,
    /// Skin-parts mask sent in `C15PacketClientSettings`.
    pub model_parts: u8,
    /// Rebound keys by description (missing = the binding's default).
    pub key_codes: std::collections::BTreeMap<String, i32>,
    pub show_debug: bool,
    pub hide_gui: bool,
    /// F3+H: item ids, damage and durability in tooltips.
    pub advanced_item_tooltips: bool,
    /// The selected item's name above the hotbar.
    pub held_item_tooltips: bool,
    /// `soundCategory_<name>` volumes (missing = 1.0).
    pub sound_volumes: std::collections::BTreeMap<String, f32>,
    /// `resourcePacks`: the selected pack names, lowest priority first.
    pub resource_packs: Vec<String>,
    /// `incompatibleResourcePacks`: selected packs whose `pack_format` is not 1.
    pub incompatible_resource_packs: Vec<String>,
    path: Option<PathBuf>,
}

impl Default for Settings {
    /// Vanilla defaults (`GameSettings` field initialisers).
    fn default() -> Self {
        Self {
            fov: 70.0,
            mouse_sensitivity: 0.5,
            gamma: 0.0,
            render_distance: 12,
            limit_framerate: 120,
            gui_scale: 0,
            ambient_occlusion: 2,
            fancy_graphics: true,
            view_bobbing: true,
            vsync: true,
            anaglyph: false,
            clouds: 2,
            particles: 0,
            fullscreen: false,
            mipmap_levels: 4,
            block_alternatives: true,
            use_vbo: false,
            entity_shadows: true,
            invert_mouse: false,
            chat_visibility: 0,
            chat_colors: true,
            chat_links: true,
            chat_links_prompt: true,
            chat_opacity: 1.0,
            chat_scale: 1.0,
            chat_width: 1.0,
            chat_height_focused: 1.0,
            chat_height_unfocused: 0.44366196,
            model_parts: 0x7f,
            key_codes: Default::default(),
            show_debug: false,
            hide_gui: false,
            advanced_item_tooltips: false,
            held_item_tooltips: true,
            sound_volumes: Default::default(),
            resource_packs: Vec::new(),
            incompatible_resource_packs: Vec::new(),
            path: None,
        }
    }
}

impl Settings {
    /// Loads `options.txt`; unknown keys are ignored and missing ones keep defaults.
    pub fn load(path: &Path) -> Self {
        let mut s = Self { path: Some(path.to_path_buf()), ..Self::default() };
        let Ok(text) = std::fs::read_to_string(path) else { return s };
        for line in text.lines() {
            let Some((k, v)) = line.split_once(':') else { continue };
            let f = v.parse::<f32>().ok();
            let i = v.parse::<i32>().ok();
            let b = v == "true";
            match k {
                // options.txt stores FOV normalized ((fov - 70) / 40).
                "fov" => s.fov = f.map_or(s.fov, |f| f * 40.0 + 70.0),
                "mouseSensitivity" => s.mouse_sensitivity = f.unwrap_or(s.mouse_sensitivity),
                "gamma" => s.gamma = f.unwrap_or(s.gamma),
                "renderDistance" => s.render_distance = i.unwrap_or(s.render_distance),
                "maxFps" => s.limit_framerate = i.unwrap_or(s.limit_framerate),
                "guiScale" => s.gui_scale = i.unwrap_or(s.gui_scale),
                "ao" => {
                    s.ambient_occlusion = match v {
                        "true" => 2,
                        "false" => 0,
                        _ => i.unwrap_or(2),
                    }
                }
                "fancyGraphics" => s.fancy_graphics = b,
                "bobView" => s.view_bobbing = b,
                "enableVsync" => s.vsync = b,
                "anaglyph3d" => s.anaglyph = b,
                "renderClouds" => {
                    s.clouds = match v {
                        "true" => 2,
                        "fast" => 1,
                        _ => 0,
                    }
                }
                "particles" => s.particles = i.unwrap_or(s.particles),
                "fullscreen" => s.fullscreen = b,
                "mipmapLevels" => s.mipmap_levels = i.unwrap_or(s.mipmap_levels),
                "allowBlockAlternatives" => s.block_alternatives = b,
                "useVbo" => s.use_vbo = b,
                "entityShadows" => s.entity_shadows = b,
                "invertYMouse" => s.invert_mouse = b,
                "chatVisibility" => s.chat_visibility = i.unwrap_or(s.chat_visibility),
                "chatColors" => s.chat_colors = b,
                "advancedItemTooltips" => s.advanced_item_tooltips = b,
                "heldItemTooltips" => s.held_item_tooltips = b,
                "chatLinks" => s.chat_links = b,
                "chatLinksPrompt" => s.chat_links_prompt = b,
                "chatOpacity" => s.chat_opacity = f.unwrap_or(s.chat_opacity),
                "chatScale" => s.chat_scale = f.unwrap_or(s.chat_scale),
                "chatWidth" => s.chat_width = f.unwrap_or(s.chat_width),
                "chatHeightFocused" => s.chat_height_focused = f.unwrap_or(s.chat_height_focused),
                "chatHeightUnfocused" => s.chat_height_unfocused = f.unwrap_or(s.chat_height_unfocused),
                "resourcePacks" => s.resource_packs = parse_list(v),
                "incompatibleResourcePacks" => s.incompatible_resource_packs = parse_list(v),
                _ if k.starts_with("key_") => {
                    if let Some(i) = i {
                        s.key_codes.insert(k[4..].to_string(), i);
                    }
                }
                _ if k.starts_with("modelPart_") => {
                    if let Some(bit) = MODEL_PARTS.iter().position(|p| *p == &k["modelPart_".len()..]) {
                        if b {
                            s.model_parts |= 1 << bit;
                        } else {
                            s.model_parts &= !(1 << bit);
                        }
                    }
                }
                _ if k.starts_with("soundCategory_") => {
                    if let Some(f) = f {
                        s.sound_volumes.insert(k["soundCategory_".len()..].to_string(), f);
                    }
                }
                _ => {}
            }
        }
        s
    }

    /// Rewrites our keys in `options.txt`, keeping every other line as it was.
    pub fn save(&self) {
        let Some(path) = &self.path else { return };
        let mut ours = vec![
            ("fov", format!("{}", (self.fov - 70.0) / 40.0)),
            ("mouseSensitivity", format!("{}", self.mouse_sensitivity)),
            ("gamma", format!("{}", self.gamma)),
            ("renderDistance", self.render_distance.to_string()),
            ("maxFps", self.limit_framerate.to_string()),
            ("guiScale", self.gui_scale.to_string()),
            ("ao", self.ambient_occlusion.to_string()),
            ("fancyGraphics", self.fancy_graphics.to_string()),
            ("bobView", self.view_bobbing.to_string()),
            ("enableVsync", self.vsync.to_string()),
            ("anaglyph3d", self.anaglyph.to_string()),
            ("renderClouds", ["false", "fast", "true"][self.clouds.clamp(0, 2) as usize].to_string()),
            ("particles", self.particles.to_string()),
            ("fullscreen", self.fullscreen.to_string()),
            ("mipmapLevels", self.mipmap_levels.to_string()),
            ("allowBlockAlternatives", self.block_alternatives.to_string()),
            ("useVbo", self.use_vbo.to_string()),
            ("entityShadows", self.entity_shadows.to_string()),
            ("invertYMouse", self.invert_mouse.to_string()),
            ("chatVisibility", self.chat_visibility.to_string()),
            ("chatColors", self.chat_colors.to_string()),
            ("advancedItemTooltips", self.advanced_item_tooltips.to_string()),
            ("heldItemTooltips", self.held_item_tooltips.to_string()),
            ("chatLinks", self.chat_links.to_string()),
            ("chatLinksPrompt", self.chat_links_prompt.to_string()),
            ("chatOpacity", self.chat_opacity.to_string()),
            ("chatScale", self.chat_scale.to_string()),
            ("chatWidth", self.chat_width.to_string()),
            ("chatHeightFocused", self.chat_height_focused.to_string()),
            ("chatHeightUnfocused", self.chat_height_unfocused.to_string()),
            ("resourcePacks", gson_list(&self.resource_packs)),
            ("incompatibleResourcePacks", gson_list(&self.incompatible_resource_packs)),
        ];
        let part_keys: Vec<String> = MODEL_PARTS.iter().map(|p| format!("modelPart_{p}")).collect();
        for (i, k) in part_keys.iter().enumerate() {
            ours.push((k.as_str(), (self.model_parts >> i & 1 != 0).to_string()));
        }
        let binding_keys: Vec<String> = crate::keys::BINDINGS.iter().map(|b| format!("key_{}", b.0)).collect();
        for (k, b) in binding_keys.iter().zip(crate::keys::BINDINGS.iter()) {
            ours.push((k.as_str(), self.key(b.0).to_string()));
        }
        let sound_keys: Vec<String> = self.sound_volumes.keys().map(|k| format!("soundCategory_{k}")).collect();
        for (k, (_, v)) in sound_keys.iter().zip(&self.sound_volumes) {
            ours.push((k.as_str(), v.to_string()));
        }
        let existing = std::fs::read_to_string(path).unwrap_or_default();
        let mut out: Vec<String> = existing
            .lines()
            .filter(|l| !ours.iter().any(|(k, _)| l.split_once(':').is_some_and(|(lk, _)| lk == *k)))
            .map(str::to_string)
            .collect();
        out.extend(ours.iter().map(|(k, v)| format!("{k}:{v}")));
        let _ = std::fs::write(path, out.join("\n") + "\n");
    }

    /// The game directory: where `options.txt` lives (`Minecraft.mcDataDir`). `resourcepacks/`
    /// and `server-resource-packs/` are inside it.
    pub fn game_dir(&self) -> PathBuf {
        match self.path.as_deref().and_then(Path::parent) {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        }
    }

    /// A binding's key code (`KeyBinding.getKeyCode`).
    pub fn key(&self, name: &str) -> i32 {
        self.key_codes.get(name).copied().unwrap_or_else(|| crate::keys::BINDINGS.iter().find(|b| b.0 == name).map_or(0, |b| b.2))
    }

    /// `getSoundLevel`
    pub fn sound_volume(&self, category: &str) -> f32 {
        self.sound_volumes.get(category).copied().unwrap_or(1.0)
    }

    pub fn float(&self, o: Opt) -> f32 {
        match o {
            Opt::Fov => self.fov,
            Opt::Sensitivity => self.mouse_sensitivity,
            Opt::Gamma => self.gamma,
            Opt::RenderDistance => self.render_distance as f32,
            Opt::FramerateLimit => self.limit_framerate as f32,
            Opt::MipmapLevels => self.mipmap_levels as f32,
            Opt::ChatOpacity => self.chat_opacity,
            Opt::ChatScale => self.chat_scale,
            Opt::ChatWidth => self.chat_width,
            Opt::ChatHeightFocused => self.chat_height_focused,
            Opt::ChatHeightUnfocused => self.chat_height_unfocused,
            Opt::SoundVolume(i) => self.sound_volume(SOUND_CATEGORIES[i as usize]),
            _ => 0.0,
        }
    }

    pub fn set_float(&mut self, o: Opt, v: f32) {
        match o {
            Opt::Fov => self.fov = v,
            Opt::Sensitivity => self.mouse_sensitivity = v,
            Opt::Gamma => self.gamma = v,
            Opt::RenderDistance => self.render_distance = v as i32,
            Opt::FramerateLimit => self.limit_framerate = v as i32,
            Opt::MipmapLevels => self.mipmap_levels = v as i32,
            Opt::ChatOpacity => self.chat_opacity = v,
            Opt::ChatScale => self.chat_scale = v,
            Opt::ChatWidth => self.chat_width = v,
            Opt::ChatHeightFocused => self.chat_height_focused = v,
            Opt::ChatHeightUnfocused => self.chat_height_unfocused = v,
            Opt::SoundVolume(i) => {
                self.sound_volumes.insert(SOUND_CATEGORIES[i as usize].to_string(), v);
            }
            _ => {}
        }
    }

    /// `setOptionValue`: cycles a button option.
    pub fn cycle(&mut self, o: Opt) {
        match o {
            Opt::GuiScale => self.gui_scale = (self.gui_scale + 1) & 3,
            Opt::AmbientOcclusion => self.ambient_occlusion = (self.ambient_occlusion + 1) % 3,
            Opt::Graphics => self.fancy_graphics = !self.fancy_graphics,
            Opt::ViewBobbing => self.view_bobbing = !self.view_bobbing,
            Opt::Vsync => self.vsync = !self.vsync,
            Opt::Anaglyph => self.anaglyph = !self.anaglyph,
            Opt::RenderClouds => self.clouds = (self.clouds + 1) % 3,
            Opt::Particles => self.particles = (self.particles + 1) % 3,
            Opt::Fullscreen => self.fullscreen = !self.fullscreen,
            Opt::BlockAlternatives => self.block_alternatives = !self.block_alternatives,
            Opt::Vbo => self.use_vbo = !self.use_vbo,
            Opt::EntityShadows => self.entity_shadows = !self.entity_shadows,
            Opt::InvertMouse => self.invert_mouse = !self.invert_mouse,
            Opt::ChatVisibility => self.chat_visibility = (self.chat_visibility + 1) % 3,
            Opt::ChatColors => self.chat_colors = !self.chat_colors,
            Opt::ChatLinks => self.chat_links = !self.chat_links,
            Opt::ChatLinksPrompt => self.chat_links_prompt = !self.chat_links_prompt,
            Opt::ModelPart(i) => self.model_parts ^= 1 << i,
            _ => {}
        }
    }

    /// `getKeyBinding`: the button label.
    pub fn label(&self, o: Opt, lang: &crate::lang::Lang) -> String {
        let s = format!("{}: ", lang.get(o.key()));
        let t = |k: &str| lang.get(k);
        if o.range().is_some() {
            let f1 = self.float(o);
            let f = o.normalize(f1);
            return match o {
                Opt::Sensitivity if f == 0.0 => s + &t("options.sensitivity.min"),
                Opt::Sensitivity if f == 1.0 => s + &t("options.sensitivity.max"),
                Opt::Sensitivity => format!("{s}{}%", (f * 200.0) as i32),
                Opt::Fov if f1 == 70.0 => s + &t("options.fov.min"),
                Opt::Fov if f1 == 110.0 => s + &t("options.fov.max"),
                Opt::Fov => format!("{s}{}", f1 as i32),
                Opt::FramerateLimit if f1 == 260.0 => s + &t("options.framerateLimit.max"),
                Opt::FramerateLimit => format!("{s}{} fps", f1 as i32),
                Opt::Gamma if f == 0.0 => s + &t("options.gamma.min"),
                Opt::Gamma if f == 1.0 => s + &t("options.gamma.max"),
                Opt::Gamma => format!("{s}+{}%", (f * 100.0) as i32),
                Opt::RenderDistance => format!("{s}{} chunks", f1 as i32),
                // GuiNewChat.calculateChatboxWidth / calculateChatboxHeight
                Opt::ChatOpacity => format!("{s}{}%", (f * 90.0 + 10.0) as i32),
                Opt::ChatScale => format!("{s}{}%", (f * 100.0) as i32),
                Opt::ChatWidth => format!("{s}{}px", (f * 280.0 + 40.0).floor() as i32),
                Opt::ChatHeightFocused | Opt::ChatHeightUnfocused => format!("{s}{}px", (f * 160.0 + 20.0).floor() as i32),
                Opt::MipmapLevels if f1 == 0.0 => s + &t("options.off"),
                Opt::MipmapLevels => format!("{s}{}", f1 as i32),
                // GuiScreenOptionsSounds.getSoundVolume
                Opt::SoundVolume(_) if f == 0.0 => s + &t("options.off"),
                Opt::SoundVolume(_) => format!("{s}{}%", (f * 100.0) as i32),
                _ => s,
            };
        }
        let on_off = |b: bool| if b { t("options.on") } else { t("options.off") };
        match o {
            Opt::GuiScale => {
                s + &t(["options.guiScale.auto", "options.guiScale.small", "options.guiScale.normal", "options.guiScale.large"]
                    [(self.gui_scale & 3) as usize])
            }
            Opt::AmbientOcclusion => {
                s + &t(["options.ao.off", "options.ao.min", "options.ao.max"][self.ambient_occlusion.clamp(0, 2) as usize])
            }
            Opt::Graphics => s + &t(if self.fancy_graphics { "options.graphics.fancy" } else { "options.graphics.fast" }),
            Opt::ViewBobbing => s + &on_off(self.view_bobbing),
            Opt::Vsync => s + &on_off(self.vsync),
            Opt::Anaglyph => s + &on_off(self.anaglyph),
            Opt::RenderClouds => s + &t(["options.off", "options.graphics.fast", "options.graphics.fancy"][self.clouds.clamp(0, 2) as usize]),
            Opt::Particles => s + &t(["options.particles.all", "options.particles.decreased", "options.particles.minimal"][self.particles.clamp(0, 2) as usize]),
            Opt::Fullscreen => s + &on_off(self.fullscreen),
            Opt::BlockAlternatives => s + &on_off(self.block_alternatives),
            Opt::Vbo => s + &on_off(self.use_vbo),
            Opt::EntityShadows => s + &on_off(self.entity_shadows),
            Opt::InvertMouse => s + &on_off(self.invert_mouse),
            Opt::ChatVisibility => s + &t(["options.chat.visibility.full", "options.chat.visibility.system", "options.chat.visibility.hidden"][self.chat_visibility.clamp(0, 2) as usize]),
            Opt::ChatColors => s + &on_off(self.chat_colors),
            Opt::ChatLinks => s + &on_off(self.chat_links),
            Opt::ChatLinksPrompt => s + &on_off(self.chat_links_prompt),
            Opt::ModelPart(i) => s + &on_off(self.model_parts >> i & 1 != 0),
            _ => s,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_pack_lists_round_trip() {
        let dir = std::env::temp_dir().join(format!("rustmc-opts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("options.txt");
        std::fs::write(&path, "fov:0.5\nresourcePacks:[\"a.zip\",\"b\"]\nincompatibleResourcePacks:[\"b\"]\nother:keep\n").unwrap();
        let mut s = Settings::load(&path);
        assert_eq!(s.resource_packs, ["a.zip", "b"]);
        assert_eq!(s.incompatible_resource_packs, ["b"]);
        assert_eq!(s.game_dir(), dir);
        s.resource_packs = vec!["it's=<x>.zip".into(), "c".into()];
        s.incompatible_resource_packs.clear();
        s.save();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("resourcePacks:[\"it\\u0027s\\u003d\\u003cx\\u003e.zip\",\"c\"]\n"), "{text}");
        assert!(text.contains("incompatibleResourcePacks:[]\n") && text.contains("other:keep\n"));
        let back = Settings::load(&path);
        assert_eq!(back.resource_packs, ["it's=<x>.zip", "c"]);
        assert!(back.incompatible_resource_packs.is_empty());
        // Vanilla's default (no keys) and malformed lists.
        std::fs::write(&path, "resourcePacks:oops\n").unwrap();
        assert!(Settings::load(&path).resource_packs.is_empty());
    }

    #[test]
    fn slider_snapping() {
        assert_eq!(Opt::Fov.denormalize(0.5), 70.0);
        assert_eq!(Opt::FramerateLimit.denormalize(1.0), 260.0);
        assert_eq!(Opt::RenderDistance.denormalize(0.0), 2.0);
        assert!((Opt::Fov.normalize(70.0) - 0.5).abs() < 1e-6);
    }
}
