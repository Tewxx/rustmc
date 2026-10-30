use anyhow::Result;
use wasmtime::{Config, Engine, Store, component::{Component, Linker}};
wasmtime::component::bindgen!({path: "../accountmanager/wit", world: "interactive-mod"});
use rustmc::plugin::{accounts, game, screens, types, ui_draw, log, input};
#[derive(Default)]
struct Host { calls: Vec<String>, refreshed: usize }
impl types::Host for Host {}
impl game::Host for Host {
    fn player(&mut self) -> game::PlayerState { let v = types::Vec3{x:0.,y:0.,z:0.}; game::PlayerState {pos:v.clone(),prev_pos:v.clone(),motion:v,yaw:0.,pitch:0.,on_ground:true,health:20.,food:20,saturation:5.,xp_level:0,selected_slot:0,sprinting:false,sneaking:false,flying:false,gamemode:0} }
    fn game_time(&mut self)->u64 {0}
}
impl ui_draw::Host for Host { fn submit(&mut self, _: Vec<u8>) {} }
impl log::Host for Host { fn log(&mut self, _: types::LogLevel, _: String) {} }
impl input::Host for Host { fn monotonic_ms(&mut self)->u64 {0} fn screen_open(&mut self)->bool {false} fn player_using_item(&mut self)->Option<bool> {Some(false)} }
impl screens::Host for Host {
 fn register(&mut self, id: String, _title: String) {self.calls.push(format!("register:{id}"));}
 fn add_entry(&mut self, _: screens::ParentScreen, id: String, _: String) {self.calls.push(format!("entry:{id}"));}
 fn refresh(&mut self, _: String) {self.refreshed+=1;}
 fn open(&mut self, _: String) {}
 fn close(&mut self) {self.calls.push("close".into());}
}
impl accounts::Host for Host {
 fn list(&mut self)->Vec<accounts::Account> {vec![accounts::Account{id:"opaque".into(),username:"Example".into(),uuid:None,kind:accounts::AccountKind::Offline,ban:accounts::BanStatus::None}, accounts::Account{id:"second".into(),username:"Second".into(),uuid:None,kind:accounts::AccountKind::Microsoft,ban:accounts::BanStatus::None}]}
 fn current(&mut self)->Option<String>{None}
 fn launch(&mut self)->Option<String>{Some("launch".into())}
 fn activate(&mut self,id:String)->std::result::Result<u32,String>{self.calls.push(format!("activate:{id}"));Ok(7)}
 fn restore_launch(&mut self)->std::result::Result<u32,String>{Ok(8)}
 fn remove(&mut self,id:String)->std::result::Result<(),String>{self.calls.push(format!("remove:{id}"));Ok(())}
 fn initiate_add(&mut self,kind:accounts::AccountKind)->std::result::Result<u32,String>{self.calls.push(format!("add:{kind:?}"));Ok(9)}
 fn operation(&mut self,_:u32)->Option<accounts::OperationState>{Some(accounts::OperationState::Complete)}
 fn move_before(&mut self,id:String,before:Option<String>)->std::result::Result<(),String>{self.calls.push(format!("move:{id}:{before:?}"));Ok(())}
 fn ban_remaining_ms(&mut self,_:String)->Option<u64>{None}
 fn set_ban_status(&mut self,_:String,_:accounts::BanStatus)->std::result::Result<(),String>{Ok(())}
}
fn main()->Result<()> {
 let mut config=Config::new(); config.wasm_component_model(true).consume_fuel(true);
 let engine=Engine::new(&config)?;
 let component=Component::from_file(&engine,std::env::args_os().nth(1).expect("component path"))?;
 let mut linker=Linker::new(&engine); InteractiveMod::add_to_linker(&mut linker, |h:&mut Host|h)?;
 let mut store=Store::new(&engine,Host::default()); store.set_fuel(20_000_000)?;
 let b=InteractiveMod::instantiate(&mut store,&component,&linker)?;
 b.rustmc_plugin_lifecycle().call_on_load(&mut store,"1.0",&[])?.map_err(anyhow::Error::msg)?;
 assert_eq!(store.data().calls.len(),3);
 let widgets=b.rustmc_plugin_screen_handlers().call_init(&mut store,"accountmanager",500,300)?;
 assert!(widgets.iter().any(|w|w.id==100 && w.label.contains("Example")));
 b.rustmc_plugin_screen_handlers().call_on_widget(&mut store,"accountmanager",100,&screens::Value::Click)?;
 b.rustmc_plugin_screen_handlers().call_on_widget(&mut store,"accountmanager",0,&screens::Value::Click)?;
 assert!(store.data().calls.contains(&"activate:opaque".to_string()));
 b.rustmc_plugin_lifecycle().call_on_tick(&mut store)?;
 let key = exports::rustmc::plugin::events::KeyEvent{code:208,down:true,repeat:false,modifiers:1,char:0};
 assert!(b.rustmc_plugin_events().call_on_key(&mut store,key)?);
 assert!(store.data().calls.contains(&"move:opaque:None".to_string()));
 b.rustmc_plugin_screen_handlers().call_on_widget(&mut store,"accountmanager",1,&screens::Value::Click)?;
 let add_widgets=b.rustmc_plugin_screen_handlers().call_init(&mut store,"accountmanager",500,300)?;
 assert!(add_widgets.iter().any(|w|w.id==8 && w.label == "Access token"));
 b.rustmc_plugin_screen_handlers().call_on_widget(&mut store,"accountmanager",8,&screens::Value::Click)?;
 assert!(store.data().calls.contains(&"add:AccountKind::AccessToken".to_string()), "{:?}", store.data().calls);
 assert!(store.data().refreshed>=5);
 println!("account guest registered screens, selected and activated opaque account, and observed operation completion");
 Ok(())
}
