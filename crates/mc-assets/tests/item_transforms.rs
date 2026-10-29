use glam::Vec3;
use mc_assets::{
    model::{ItemTransform, ModelLoader},
    AssetSource,
};

#[test]
fn all_model_display_transforms_match_java() {
    let (Some(golden), Some(jar)) = (mc_testutil::golden("item_transforms.json"), mc_testutil::jar()) else {
        return;
    };
    let data: serde_json::Value = serde_json::from_slice(&std::fs::read(golden).unwrap()).unwrap();
    let mut assets = AssetSource::new();
    assets.push(&jar).unwrap();
    let mut loader = ModelLoader::new(&assets);
    for row in data.as_array().unwrap() {
        let name = row["model"].as_str().unwrap();
        let model = loader.get(name).unwrap_or_else(|| panic!("{name}"));
        for (kind, t) in row["display"].as_object().unwrap() {
            let actual = model.display.get(kind).copied().unwrap_or_default();
            let v = |key: &str| Vec3::from_array(std::array::from_fn(|i| t[key][i].as_f64().unwrap() as f32));
            let expected = ItemTransform {
                rotation: v("rotation"),
                translation: v("translation"),
                scale: v("scale"),
            };
            assert_eq!(actual, expected, "{name} {kind}");
        }
    }
}
