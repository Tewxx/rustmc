use anyhow::Result;
use wasmtime::{Config, Engine, Store, component::{Component, Linker}};
wasmtime::component::bindgen!({path: "../lunarforge-input/wit", world: "input-mod"});
use rustmc::plugin::{game,input,types,ui_draw,log};
#[derive(Default)] struct Host { now:u64, screen:bool, using:Option<bool>, draw:Vec<u8> }
impl types::Host for Host {}
impl game::Host for Host {
 fn player(&mut self)->game::PlayerState {let v=types::Vec3{x:0.,y:0.,z:0.}; game::PlayerState{pos:v.clone(),prev_pos:v.clone(),motion:v,yaw:0.,pitch:0.,on_ground:true,health:20.,food:20,saturation:5.,xp_level:0,selected_slot:0,sprinting:false,sneaking:false,flying:false,gamemode:0}}
 fn game_time(&mut self)->u64 {0}
}
impl input::Host for Host {fn monotonic_ms(&mut self)->u64 {self.now} fn screen_open(&mut self)->bool {self.screen} fn player_using_item(&mut self)->Option<bool>{self.using}}
impl ui_draw::Host for Host {fn submit(&mut self,b:Vec<u8>){self.draw=b;}}
impl log::Host for Host {fn log(&mut self,_:types::LogLevel,_:String){}}
fn main()->Result<()> {
 let mut c=Config::new(); c.wasm_component_model(true).consume_fuel(true);
 let engine=Engine::new(&c)?; let component=Component::from_file(&engine,std::env::args_os().nth(1).expect("component path"))?;
 let mut linker=Linker::new(&engine); InputMod::add_to_linker(&mut linker,|h:&mut Host|h)?;
 let mut store=Store::new(&engine,Host{now:1000,screen:false,using:Some(false),draw:vec![]});store.set_fuel(20_000_000)?;
 let b=InputMod::instantiate(&mut store,&component,&linker)?;
 b.rustmc_plugin_lifecycle().call_on_load(&mut store,"1.0",&[])?.map_err(anyhow::Error::msg)?;
 let evt=exports::rustmc::plugin::events::MouseEvent{x:0,y:0,button:0,down:true,wheel:0};
 assert!(!b.rustmc_plugin_events().call_on_mouse(&mut store,evt)?);
 let v=types::Vec3{x:0.,y:0.,z:0.};let frame=types::FrameCtx{partial_ticks:0.,fps:60,gui_width:854,gui_height:480,gui_scale:1,mouse_x:0,mouse_y:0,cam:v,cam_yaw:0.,cam_pitch:0.,fov:70.};
 b.rustmc_plugin_render().call_render_hud(&mut store,frame.clone())?;
 assert!(store.data().draw.ends_with(b"1 CPS"));
 store.data_mut().now=2001;
 b.rustmc_plugin_render().call_render_hud(&mut store,frame.clone())?;
 assert!(store.data().draw.ends_with(b"0 CPS"));
 println!("CPS guest receives input, renders one click, and expires it after 1000 ms"); Ok(())
}
