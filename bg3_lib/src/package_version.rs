#[derive(Debug, Default)]
pub enum PackageVersion {
    #[default]
    None,
    V18 = 18,
}

impl TryFrom<i32> for PackageVersion {
    type Error = String;

    fn try_from(value: i32) -> Result<Self, Self::Error> {
        match value {
            18 => Ok(Self::V18),
            _ => Err(format!("i32 value '{value}' is not a valid version")),
        }
    }
}
