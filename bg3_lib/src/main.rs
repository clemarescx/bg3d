use bg3_lib::package_reader::PackageReader;
use std::path::Path;

fn main() {
    let path_arg = std::env::args()
        .nth(1)
        .expect("usage: <exec> <path to .lsv file>");
    let path = Path::new(&path_arg);
    let mut package_reader = PackageReader::new(path).unwrap();
    let package = package_reader.read().unwrap();
    let all_resources = package_reader.load_all(&package).unwrap();
    println!("resources count: {}", all_resources.len());
}
