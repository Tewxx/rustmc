//! Controlled JVM/Mixin integration: this test runs no third-party client code.
use jni::{objects::{JObject, JValue}, JNIEnv};
use mc_forge::{game, hooks, impls, live, runtime};
use serde_json::{json, Value};
use std::path::PathBuf;

fn entity(id:i32,class:&str,name:&str,x:f64)->Value {
    json!({"id":id,"class":class,"name":name,"uuid":format!("00000000-0000-0000-0000-{id:012}"),
        "x":x,"y":64.,"z":-2.,"prev_x":x-0.1,"prev_y":64.,"prev_z":-2.,"last_x":x-0.1,"last_y":64.,"last_z":-2.,
        "motion_x":0.,"motion_y":0.,"motion_z":0.,"yaw":0.,"pitch":0.,"prev_yaw":0.,"prev_pitch":0.,
        "flags":0,"on_ground":true,"dead":false,"no_clip":false,"ticks":5,"width":0.6,"height":1.8,"fall_distance":0.,"health":19.,"hurt_time":0})
}
fn snapshot()->Value {
    let mut inventory=vec![Value::Null;40];
    inventory[0]=json!({"id":276,"class":"net.minecraft.item.ItemSword","name":"minecraft:diamond_sword","max":1,"count":1,"damage":12,"nbt":null});
    json!({"generation":1,"active":true,"dimension":0,"time":6000,"total_time":18000,"partial_ticks":0.5,
        "entities":[entity(1,"net.minecraft.client.entity.EntityPlayerSP","Local",1.),entity(2,"net.minecraft.client.entity.EntityOtherPlayerMP","Remote",3.)],
        "selected_slot":0,"inventory":inventory,"input":{"strafe":0.,"forward":0.,"jump":false,"sneak":false},
        "capabilities":{"flying":false,"allow_flying":false,"creative":false,"invulnerable":false},
        "settings":{"third_person":0,"fov":70.,"sensitivity":0.5,"render_distance":8}})
}
fn probe(env:&mut JNIEnv<'_>,method:&str,sig:&str,args:&[JValue<'_, '_>]) {
    runtime::call_static(env,"ft/liveprobe/Probe",method,sig,args).unwrap_or_else(|e|{let _=env.exception_describe();panic!("{method}: {e}");});
}
fn trace(env:&mut JNIEnv<'_>)->String {
    let s=runtime::call_static(env,"ft/liveprobe/Probe","trace","()Ljava/lang/String;",&[]).unwrap().l().unwrap();
    env.get_string(&s.into()).unwrap().into()
}

#[test]
fn live_graph_actions_and_java_hooks_reach_the_real_rust_body() {
    std::thread::Builder::new().stack_size(64<<20).spawn(flow).unwrap().join().unwrap();
}
fn flow(){
    let root=PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path=root.join("tests/forge/build/runtime.json");
    let probe_path=root.join("tests/forge/build/liveprobe-runtime.json");
    if !path.is_file()||!probe_path.is_file(){
        assert_ne!(std::env::var("RUSTMC_REQUIRE_FORGE").as_deref(),Ok("1"),"build tools/shadow/stubs.ps1 and tests/mods/liveprobe/build.ps1 first");
        eprintln!("skipped: local Forge liveprobe inputs absent");return;
    }
    let config:Value=serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let fixture:Value=serde_json::from_slice(&std::fs::read(probe_path).unwrap()).unwrap();
    let jars=vec![PathBuf::from(fixture["probeJar"].as_str().unwrap())];
    let mut cp=vec![PathBuf::from(config["stubsJar"].as_str().unwrap()),jars[0].clone()];
    cp.extend(std::env::split_paths(fixture["classpath"].as_str().unwrap()));
    runtime::start(&PathBuf::from(config["libjvm"].as_str().unwrap()),&cp,&jars).unwrap();
    runtime::with_env(|env|{
        let mut state=snapshot();
        probe(env,"listen","()V",&[]);
        live::publish(env,&state).unwrap_or_else(|e|{let _=env.exception_describe();panic!("publish: {e}");});
        assert_eq!(trace(env),"load,join:1,join:2","world load precedes joins, in entity order");
        probe(env,"clearTrace","()V",&[]);
        state["view"]=json!({"x":1.,"y":64.,"z":-2.,"yaw":45.,"pitch":10.,
            "target":{"type":"entity","id":2,"hx":3.,"hy":65.,"hz":-2.}});
        live::publish(env,&state).unwrap();
        probe(env,"assertView","(DDDFI)V",&[JValue::Double(1.),JValue::Double(64.),JValue::Double(-2.),JValue::Float(45.),JValue::Int(2)]);
        assert_eq!(trace(env),"","a stable snapshot fires no lifecycle events");
        game::player_tick(env,true).unwrap();
        assert!(!game::living_update(env).unwrap());
        game::player_tick(env,false).unwrap();
        assert!(!game::attack_entity(env,2).unwrap());
        probe(env,"setCancelAttack","(Z)V",&[JValue::Bool(1)]);
        assert!(game::attack_entity(env,2).unwrap(),"a cancelled AttackEntityEvent reports cancellation");
        assert!(!game::attack_entity(env,99).unwrap(),"unknown targets post nothing");
        assert_eq!(trace(env),"player:START,living,player:END,attack:2,attack:2");
        probe(env,"resetTrace","()V",&[]);
        probe(env,"assertGraph","(IIDFIII)V",&[JValue::Int(1),JValue::Int(2),JValue::Double(1.),JValue::Float(19.),JValue::Int(276),JValue::Int(1),JValue::Int(12)]);
        probe(env,"remember","(I)V",&[JValue::Int(2)]);
        live::publish(env,&state).unwrap();probe(env,"assertStable","(I)V",&[JValue::Int(2)]);
        assert_eq!(live::changes(env).unwrap()["player"],json!({}),"unchanged Java fields must not overwrite new Rust input");
        probe(env,"mutateAndAct","(I)V",&[JValue::Int(2)]);
        let changes=live::changes(env).unwrap();
        assert_eq!(changes["player"]["yaw"],137.5);assert_eq!(changes["player"]["sprinting"],true);
        assert_eq!(changes["player"]["selected_slot"],7);
        assert!(changes["player"].get("x").is_none());
        let actions=changes["actions"].as_array().unwrap();
        assert!(actions.iter().any(|a|a["kind"]=="attack"&&a["values"][0]==2));
        assert_eq!(actions.iter().filter(|a|a["kind"]=="packet").count(),2);
        let packet_ids:Vec<_>=actions.iter().filter(|a|a["kind"]=="packet").map(|a|a["values"][0].as_i64().unwrap() as u8).collect();
        assert_eq!(packet_ids,vec![1,4]);
        assert_eq!(live::changes(env).unwrap()["player"],json!({}),"deltas must drain once");

        let mc=impls::minecraft(env).unwrap();
        let player=env.get_field(&mc,"field_71439_g","Lnet/minecraft/client/entity/EntityPlayerSP;").unwrap().l().unwrap();
        for (object,method,label,cancel) in [(&mc,"func_71407_l","tick","setCancelTick"),(&player,"func_175161_p","walk","setCancelWalking")]{
            probe(env,"resetTrace","()V",&[]);
            let ran=hooks::call(env,object,method,&mut ||{runtime::with_env(|e|probe(e,"body","()V",&[]));}).unwrap();
            assert!(ran);assert_eq!(trace(env),format!("{label}:head,body,{label}:return"));
            probe(env,"clearTrace","()V",&[]);probe(env,cancel,"(Z)V",&[JValue::Bool(1)]);
            let mut count=0;assert!(!hooks::call(env,object,method,&mut ||count+=1).unwrap());assert_eq!(count,0);
            probe(env,cancel,"(Z)V",&[JValue::Bool(0)]);
        }
        state["entities"].as_array_mut().unwrap().pop();
        live::publish(env,&state).unwrap();probe(env,"assertRemoved","(I)V",&[JValue::Int(2)]);
        probe(env,"resetTrace","()V",&[]);
        let hello=r#"{"text":"hello"}"#;
        assert_eq!(game::chat_received(env,0,hello).unwrap().as_deref(),Some(hello),"an untouched message keeps its JSON");
        assert_eq!(game::chat_received(env,1,r#"{"text":"hide me"}"#).unwrap(),None);
        assert!(game::chat_received(env,0,r#"{"text":"edit me"}"#).unwrap().unwrap().contains("edited"));
        assert!(!game::overlay(env,0.5,true,"ALL").unwrap());
        let (left,right)=game::overlay_text(env,0.5,&[],&[]).unwrap().unwrap();
        assert_eq!(left,vec!["LiveProbe HUD".to_string()]);assert!(right.is_empty());
        game::overlay(env,0.5,false,"ALL").unwrap();
        impls::set_key(19,true,Some('r'),false);impls::key_bindings(env,19,true).unwrap();game::key_input(env).unwrap();
        let s=runtime::call_static(env,"ft/liveprobe/Probe","keyState","()Ljava/lang/String;",&[]).unwrap().l().unwrap();
        let keys:String=env.get_string(&s.into()).unwrap().into();
        assert_eq!(keys,"19:key.liveprobe:true:pressed","one press queues exactly one isPressed");
        impls::set_key(19,false,None,false);impls::key_bindings(env,19,false).unwrap();game::key_input(env).unwrap();
        assert_eq!(trace(env),"chat:0:hello,chat:1:hide me,chat:0:edit me,pre:ALL,text,post:ALL,key:19:true:true,key:19:false:false");
        // World and screen transitions go through Java's own methods, so their Mixin handlers run.
        probe(env,"resetTrace","()V",&[]);
        assert!(hooks::load_world(env).unwrap(),"loadWorld is a Rust-owned continuation");
        assert_eq!(trace(env),"load:world");
        probe(env,"resetTrace","()V",&[]);
        game::open_shadow(env,"main_menu").unwrap();
        assert_eq!(trace(env),"screen:GuiMainMenu","the native twin opens through displayGuiScreen");
        assert_eq!(game::focus(env).unwrap(),game::Focus::Shadow);
        probe(env,"resetTrace","()V",&[]);
        game::close_shadow(env).unwrap();
        assert_eq!(trace(env),"screen:null");
        assert_eq!(game::focus(env).unwrap(),game::Focus::Nothing);
        probe(env,"clearTrace","()V",&[]);
        state["active"]=json!(false);live::publish(env,&state).unwrap();probe(env,"assertDisconnected","()V",&[]);
        assert_eq!(trace(env),"unload");
        // Native entry points are still available, without retaining stale Java views.
        let _=JObject::null();
    }).unwrap();
}
