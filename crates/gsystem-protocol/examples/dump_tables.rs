//! Print the parameter tables as JSON (used to cross-check the port).
fn main() {
    println!("{}", gsystem_protocol::params::dump_tables());
}
