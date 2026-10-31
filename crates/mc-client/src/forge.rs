//! The unmodded client's supported Forge registry surface. Never accept IDs we cannot interpret.
use mc_protocol::forge::Registry;
use std::collections::BTreeMap;

pub fn client_mods() -> BTreeMap<String, String> {
    [("mcp", "9.19"), ("FML", "8.0.99.99"), ("Forge", "11.15.1.2318")]
        .into_iter().map(|(k,v)| (k.to_string(), v.to_string())).collect()
}

pub fn validate(registries: &BTreeMap<String, Registry>) -> Result<(), String> {
    const POTIONS: [&str;23] = ["speed", "slowness", "haste", "mining_fatigue", "strength", "instant_health", "instant_damage", "jump_boost", "nausea", "regeneration", "resistance", "fire_resistance", "water_breathing", "invisibility", "blindness", "night_vision", "hunger", "weakness", "poison", "wither", "health_boost", "absorption", "saturation"];
    const VILLAGERS: [&str;5] = ["farmer", "librarian", "priest", "smith", "butcher"];
    for needed in ["minecraft:blocks", "minecraft:items", "minecraft:potions", "minecraft:villagerprofessions"] {
        if !registries.contains_key(needed) { return Err(format!("Forge server omitted registry {needed}")); }
    }
    for (name, registry) in registries {
        let expected = match name.as_str() {
            "minecraft:blocks" => mc_data::block::Block::all().len(),
            "minecraft:items" => mc_data::item::Item::all().len(),
            "minecraft:potions" => POTIONS.len(),
            "minecraft:villagerprofessions" => VILLAGERS.len(),
            _ => return Err(format!("Unsupported Forge registry {name}")),
        };
        if registry.ids.len() != expected { return Err(format!("Forge registry {name} has {} entries, expected {expected}", registry.ids.len())); }
        if !registry.substitutions.is_empty() || !registry.dummied.is_empty() {
            return Err(format!("Forge registry {name} needs substitutions or dummy entries"));
        }
        for (key, &id) in &registry.ids {
            let local = match name.as_str() {
                "minecraft:blocks" => mc_data::block::Block::by_name(key).map(|b| b.id as i32),
                "minecraft:items" => mc_data::item::Item::by_name(key).map(|i| i.id as i32),
                "minecraft:potions" => POTIONS.iter().position(|n| key == &format!("minecraft:{n}")).map(|i| i as i32 + 1),
                "minecraft:villagerprofessions" => VILLAGERS.iter().position(|n| key == &format!("minecraft:{n}")).map(|i| i as i32),
                _ => return Err(format!("Unsupported Forge registry {name}")),
            };
            if local != Some(id) { return Err(format!("Unsupported Forge mapping {name}: {key} = {id} (local {local:?}); mod loading and ID remapping are not yet available")); }
        }
    }
    Ok(())
}

/// Forge's fluid ID map is only supported for the two vanilla fluids for now.
pub fn validate_runtime(data: &[u8]) -> Result<(), String> {
    let mut r = mc_protocol::Reader::new(data);
    let tag = r.u8().map_err(|e| e.to_string())?;
    if tag != 2 { return Err(format!("unsupported Forge runtime message {tag}")); }
    let count = r.i32().map_err(|e| e.to_string())?;
    if count != 2 { return Err(format!("unsupported Forge fluid count {count}")); }
    let mut ids = BTreeMap::new();
    for _ in 0..count {
        let name = r.string(255).map_err(|e| e.to_string())?;
        let id = r.i32().map_err(|e| e.to_string())?;
        if ids.insert(name,id).is_some() { return Err("duplicate Forge fluid".into()); }
    }
    if ids.get("water") != Some(&1) || ids.get("lava") != Some(&2) { return Err("unsupported Forge fluid IDs".into()); }
    if r.remaining() > 0 {
        let mut defaults = std::collections::BTreeSet::new();
        for _ in 0..count { defaults.insert(r.string(255).map_err(|e| e.to_string())?); }
        if defaults != ["minecraft:water".to_string(), "minecraft:lava".to_string()].into_iter().collect() { return Err("unsupported Forge default fluids".into()); }
    }
    if r.remaining() != 0 { return Err("trailing Forge fluid data".into()); }
    Ok(())
}

/// Starts the embedded JVM when `RUSTMC_FORGE_STUBS` names the jar built by `tools/shadow/stubs.sh`, then runs
/// every `@Mod` in `mods-forge/*.jar`. `RUSTMC_FORGE_CP` adds Minecraft's libraries (guava, log4j, ...) to the class
/// path. Returns whether mods are running; any failure leaves the client running without Java mods.
/// Mods see the player's name and UUID, never the access token.
pub fn start_runtime(font: &mc_gui::font::FontRenderer, lang: &mc_gui::lang::Lang, name: &str, uuid: Option<&str>) -> bool {
    let Some(stubs) = std::env::var_os("RUSTMC_FORGE_STUBS") else { return false };
    let Some(libjvm) = mc_forge::runtime::find_libjvm() else {
        eprintln!("forge runtime disabled: set JAVA_HOME to a JDK 8 or RUSTMC_JVM to its libjvm");
        return false;
    };
    let jars: Vec<_> = std::fs::read_dir("mods-forge").into_iter().flatten().flatten().map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "jar")).collect();
    for jar in &jars { if let Err(e) = mc_assets::register_mod_jar(jar) { eprintln!("mod assets unavailable from {}: {e}", jar.display()); } }
    let mut classpath = vec![std::path::PathBuf::from(stubs)];
    classpath.extend(jars.iter().cloned());
    classpath.extend(std::env::var_os("RUSTMC_FORGE_CP").iter().flat_map(std::env::split_paths));
    if let Err(e) = mc_forge::runtime::start(&libjvm, &classpath, &jars) {
        eprintln!("forge runtime disabled: {e}");
        return false;
    }
    let font = font.clone();
    mc_forge::impls::set_text_width(move |s| font.width(s));
    mc_forge::impls::set_clipboard(mc_gui::clipboard::get, mc_gui::clipboard::set);
    mc_forge::impls::set_session(name, uuid.unwrap_or(""));
    mc_forge::impls::set_translations(lang.table().clone());
    match mc_forge::runtime::load_mods(&jars) {
        Ok(ids) => eprintln!("forge runtime started; mods: {ids:?}"),
        Err(e) => eprintln!("forge mods failed to load: {e}"),
    }
    // Mixin-based mods (Meowtils) initialise from Minecraft.startGame's TAIL.
    if let Some(Err(e)) = mc_forge::hooks::start_game() {
        mc_forge::runtime::with_env(|env| { let _ = env.exception_describe(); let _ = env.exception_clear(); });
        eprintln!("forge startGame hook failed: {e}");
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reject_missing_registries_and_unsupported_fluids() {
        assert!(validate(&BTreeMap::new()).is_err());
        let fluid = [2,0,0,0,2,5,b'w',b'a',b't',b'e',b'r',0,0,0,1,4,b'l',b'a',b'v',b'a',0,0,0,2];
        assert!(validate_runtime(&fluid).is_ok());
        let mut bad = fluid;
        *bad.last_mut().unwrap() = 3;
        assert!(validate_runtime(&bad).is_err());
        assert!(validate_runtime(&[1,0,0,0,0,0,0,0,0]).is_err());
    }
}
