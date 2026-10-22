//! Commands execute against the same local player, inventory and world used by the client.
use crate::{inventory::Catalogue, player::Player};
use mc_core::{BlockPos, Vec3};
use mc_data::Block;
use mc_gui::{ItemStack, Ui};
use mc_world::World;

pub struct Outcome {
    pub text: String,
    pub edits: Vec<BlockPos>,
}
pub fn execute(
    text: &str,
    player: &mut Player,
    world: &mut World,
    ui: &mut Ui,
    items: &Catalogue,
    _spawn: Vec3,
) -> Result<Outcome, String> {
    let a: Vec<_> = text.trim_start_matches('/').split_whitespace().collect();
    let mut edits = Vec::new();
    let usage = |s: &str| Err(format!("Usage: {s}"));
    let result=match a.first().copied().unwrap_or("") {
        "help"|"?"=>"/gamemode <creative|survival>, /tp <x y z>, /give [Player] <item> [count] [data], /setblock <x y z> <block> [data], /fill <x1 y1 z1 x2 y2 z2> <block> [data], /clear, /kill".into(),
        "gamemode"|"gm"=> {
            match a.get(1).copied() {Some("1"|"c"|"creative")=>{player.creative=true;},Some("0"|"s"|"survival")=>{player.creative=false;player.flying=false;},_=>return usage("/gamemode <creative|survival>")}
            format!("Set own game mode to {}",if player.creative {"Creative Mode"} else {"Survival Mode"})
        },
        "tp"|"teleport"=>{
            let args=if a.len()==5 && matches!(a[1],"Player"|"@p"|"@s") {&a[2..]} else {&a[1..]};
            if args.len()!=3 {return usage("/tp <x> <y> <z>");}
            let p=Vec3::new(coord(args[0],player.pos.x)?,coord(args[1],player.pos.y)?,coord(args[2],player.pos.z)?);
            validate(world,BlockPos::new(p.x.floor() as i32,p.y.floor() as i32,p.z.floor() as i32))?;
            player.teleport(p);format!("Teleported Player to {:.2}, {:.2}, {:.2}",p.x,p.y,p.z)
        },
        "give"=>{
            let args=if a.get(1).is_some_and(|s|matches!(*s,"Player"|"@p"|"@s")) {&a[2..]} else {&a[1..]};
            if args.is_empty() || args.len()>3 {return usage("/give [Player] <item> [count] [data]");}
            let count=args.get(1).map_or(Ok(1),|s|s.parse::<u16>().map_err(|_|"Invalid count".to_string()))?;
            if count==0 || count>2304 {return Err("Count must be between 1 and 2304".into());}
            let meta=args.get(2).map_or(Ok(0),|s|s.parse::<u16>().map_err(|_|"Invalid item data".to_string()))?;
            let item=items.find(args[0],meta).ok_or_else(||format!("Unknown item or variant: {} {meta}",args[0]))?;
            let max=items.items[item].max;
            let mut inventory=ui.inventory;let mut left=count;
            for stack in inventory.iter_mut().flatten() {if stack.item==item {let n=left.min(max.saturating_sub(stack.count) as u16);stack.count+=n as u8;left-=n;}}
            for slot in &mut inventory {if slot.is_none() && left>0 {let n=left.min(max as u16) as u8;*slot=Some(ItemStack{item,count:n});left-=n as u16;}}
            if left>0 {return Err("Inventory is full".into());}ui.inventory=inventory;
            format!("Given [{0}] * {count} to Player",items.items[item].title)
        },
        "clear"=>{ui.inventory.fill(None);"Cleared the inventory of Player".into()},
        "kill"=>{player.dead=true;"Killed Player".into()},
        "setblock"|"fill"=>{
            let fill=a[0]=="fill";let n=if fill {6} else {3};
            if a.len()<n+2 || a.len()>n+3 {return usage(if fill {"/fill <x1 y1 z1 x2 y2 z2> <block> [data]"} else {"/setblock <x y z> <block> [data]"});}
            let block=Block::by_name(a[n+1]).or_else(||a[n+1].parse::<u16>().ok().and_then(Block::by_id)).ok_or_else(||format!("Unknown block: {}",a[n+1]))?;
            let meta=a.get(n+2).map_or(Ok(0),|s|s.parse::<u8>().map_err(|_|"Invalid block data".to_string()))?;
            if meta>15 || block.state_from_meta(meta).is_none() {return Err("Invalid block data".into());}
            let pos=|start:usize|->Result<BlockPos,String>{Ok(BlockPos::new(coord(a[start],player.pos.x)?.floor() as i32,coord(a[start+1],player.pos.y)?.floor() as i32,coord(a[start+2],player.pos.z)?.floor() as i32))};
            let p=pos(1)?;let q=if fill {pos(4)?} else {p};
            let (lo,hi)=(BlockPos::new(p.x.min(q.x),p.y.min(q.y),p.z.min(q.z)),BlockPos::new(p.x.max(q.x),p.y.max(q.y),p.z.max(q.z)));
            let volume=(hi.x as i64-lo.x as i64+1)*(hi.y as i64-lo.y as i64+1)*(hi.z as i64-lo.z as i64+1);
            if volume>32768 {return Err(format!("Too many blocks in the specified area ({volume} > 32768)"));}
            for x in lo.x..=hi.x {for z in lo.z..=hi.z {for y in lo.y..=hi.y {let p=BlockPos::new(x,y,z);validate(world,p)?;edits.push(p);}}}
            for &p in &edits {world.set_block(p,block.id,meta);}
            format!("{} block(s) changed",edits.len())
        },
        _=>return Err("Unknown command. Try /help for a list of commands".into()),
    };
    Ok(Outcome { text: result, edits })
}
fn coord(s: &str, base: f64) -> Result<f64, String> {
    let (s, offset) = s.strip_prefix('~').map_or((s, 0.), |r| (r, base));
    let v = if s.is_empty() {
        0.
    } else {
        s.parse::<f64>().map_err(|_| "Invalid coordinate".to_string())?
    };
    let v = v + offset;
    if !v.is_finite() || v.abs() > 30_000_000. {
        return Err("Coordinate is outside the world".into());
    }
    Ok(v)
}
fn validate(world: &World, p: BlockPos) -> Result<(), String> {
    if !(0..256).contains(&p.y) {
        Err("Position is outside world height".into())
    } else if !world.chunks.contains_key(&(p.x >> 4, p.z >> 4)) {
        Err("That position is outside the loaded world".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coordinates_reject_nonfinite_and_support_relative() {
        assert_eq!(coord("~-2.5", 10.).unwrap(), 7.5);
        assert_eq!(coord("~", 3.).unwrap(), 3.);
        assert!(coord("NaN", 0.).is_err());
        assert!(coord("inf", 0.).is_err());
    }
    #[test]
    fn commands_change_real_state_and_reject_partial_edits() {
        let Some(jar) = crate::find_jar() else {
            return;
        };
        let mut assets = mc_assets::AssetSource::new();
        assets.push(&jar).unwrap();
        let mut ui = Ui::new(&assets, mc_gui::settings::Settings::default());
        let items = Catalogue::metadata();
        let mut world = World::default();
        world.insert(mc_world::chunk::Chunk::new(0, 0));
        let spawn = Vec3::new(8., 2., 8.);
        let mut player = Player::new(spawn);
        let mut run = |s: &str, p: &mut Player, w: &mut World| execute(s, p, w, &mut ui, &items, spawn);
        run("/give Player stone 64 2", &mut player, &mut world).unwrap();
        run("/gamemode 0", &mut player, &mut world).unwrap();
        assert!(!player.creative);
        run("/tp ~1 ~2 ~-1", &mut player, &mut world).unwrap();
        assert_eq!(player.pos, Vec3::new(9., 4., 7.));
        let result = run("/fill 1 1 1 3 1 3 planks 2", &mut player, &mut world).unwrap();
        assert_eq!(result.edits.len(), 9);
        assert_eq!(world.get(3, 1, 3), (5, 2));
        assert!(run("/fill 14 2 14 17 2 17 stone", &mut player, &mut world).is_err());
        assert_eq!(world.get(14, 2, 14), (0, 0));
        assert!(run("/setblock 1 256 1 stone", &mut player, &mut world).is_err());
        assert!(run("/give Player fake_item", &mut player, &mut world).is_err());
        run("/kill", &mut player, &mut world).unwrap();
        assert!(player.dead);
        assert_eq!(ui.inventory[0].unwrap().count, 64);
        assert_eq!(items.items[ui.inventory[0].unwrap().item].meta, 2);
        execute("/clear", &mut player, &mut world, &mut ui, &items, spawn).unwrap();
        assert!(ui.inventory.iter().all(Option::is_none));
    }
}
