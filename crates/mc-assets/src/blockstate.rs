//! `blockstates/*.json` (1.8 format: `variants` only, no multipart).

use crate::AssetSource;
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Variant {
    /// Model name relative to `models/block/`.
    pub model: String,
    pub x: i32,
    pub y: i32,
    pub uvlock: bool,
    pub weight: i32,
}

#[derive(Clone, Debug, Default)]
pub struct BlockStateDef {
    /// Variant key (e.g. `axis=y`, `normal`) to weighted alternatives, in file order.
    pub variants: BTreeMap<String, Vec<Variant>>,
}

impl BlockStateDef {
    pub fn load(assets: &AssetSource, name: &str) -> Option<Self> {
        let json = assets.read_json(&format!("blockstates/{name}.json"))?;
        let mut variants = BTreeMap::new();
        for (key, v) in json.get("variants")?.as_object()? {
            let list = match v {
                Value::Array(a) => a.iter().filter_map(parse_variant).collect(),
                other => parse_variant(other).into_iter().collect(),
            };
            variants.insert(key.clone(), list);
        }
        Some(Self { variants })
    }
}

fn parse_variant(v: &Value) -> Option<Variant> {
    let int = |k: &str| v.get(k).and_then(Value::as_i64).unwrap_or(0) as i32;
    Some(Variant {
        model: v.get("model")?.as_str()?.to_string(),
        x: int("x"),
        y: int("y"),
        uvlock: v.get("uvlock").and_then(Value::as_bool).unwrap_or(false),
        weight: v.get("weight").and_then(Value::as_i64).unwrap_or(1) as i32,
    })
}
