//! Prints every Rust-implemented stub key, one per line (`tools/shadow/gaps.py` diffs it against mod scans).
fn main() {
    mc_forge::impls::register_all();
    for key in mc_forge::stubs::registered() { println!("{key}"); }
}
