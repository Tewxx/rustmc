use mc_render::sky::Sky;
#[test]
fn overworld_sky_values_match_java() {
    let Some(path) = mc_testutil::golden("sky.json") else {
        return;
    };
    let data: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for row in data.as_array().unwrap() {
        let time = row["time"].as_i64().unwrap();
        let partial = row["partial"].as_f64().unwrap() as f32;
        let temp = row["temperature"].as_f64().unwrap() as f32;
        let sky = Sky::overworld(time, partial, temp, 32); // at 32 chunks the sky/fog blend factor is zero
        let eq = |actual: f32, expected: f32, label: &str| {
            assert_eq!(
                actual.to_bits(),
                expected.to_bits(),
                "{time} {partial} {temp} {label}: {actual} {expected}"
            )
        };
        eq(sky.angle, row["angle"].as_f64().unwrap() as f32, "angle");
        eq(sky.stars, row["stars"].as_f64().unwrap() as f32, "stars");
        for (name, actual) in [("sky", sky.sky), ("cloud", sky.cloud), ("fog", sky.fog)] {
            for i in 0..3 {
                eq(actual[i], row[name][i].as_f64().unwrap() as f32, name);
            }
        }
        let sunrise = row["sunrise"].as_array().unwrap();
        assert_eq!(sky.sunrise.is_some(), !sunrise.is_empty());
        if let Some(a) = sky.sunrise {
            for i in 0..4 {
                eq(a[i], sunrise[i].as_f64().unwrap() as f32, "sunrise");
            }
        }
        assert_eq!(sky.moon, row["moon"].as_u64().unwrap() as u8);
    }
}
