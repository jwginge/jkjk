use fmt_ja::game_path;
fn main() {
    println!("sky   = {:?}", game_path("textures/skies/hoth"));
    println!("snow  = {:?}", game_path("textures/hoth/snow"));
    println!("caulk = {:?}", game_path("textures/common/caulk"));
}
