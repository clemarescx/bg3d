use bg3_lib::{lsf_reader::Resource, package_reader::PackageReader};
use std::path::Path;

fn main() {
    let path_arg = std::env::args()
        .nth(1)
        .expect("usage: <exec> <path to .lsv file>");
    let path = Path::new(&path_arg);
    let mut package_reader = PackageReader::new(path).unwrap();
    let package = package_reader.read().unwrap();
    let resources: Resource = package_reader.load_globals(&package).unwrap();
    println!("regions count: {}", resources.regions.len());
}
