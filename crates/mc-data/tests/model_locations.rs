use mc_data::Block;
use serde_json::Value;
use std::path::Path;

#[test]
fn every_model_location_matches_java() {
    let Some(path) = mc_testutil::golden("models.json") else {
        return;
    };
    let text = std::fs::read_to_string(&path).unwrap();
    let rows: Vec<Value> = serde_json::from_str(&text).unwrap();
    let states: Vec<_> = Block::all().iter().flat_map(Block::states).collect();
    assert_eq!(rows.len(), states.len());
    for (row, state) in rows.iter().zip(states) {
        assert_eq!(row["state"].as_str().unwrap(), state.to_string());
        match state.model_location() {
            Some(loc) => {
                assert_eq!(row["name"], loc.name, "{state}");
                assert_eq!(row["variant"], loc.variant, "{state}");
            }
            None => assert!(row.get("name").is_none(), "{state}"),
        }
    }
}
