pub struct PackageMetadata {
    pub flags: u8,
    pub priority: u8,
}

impl PackageMetadata {
    pub fn new() -> PackageMetadata {
        Self {
            flags: 0,
            priority: 0,
        }
    }
}

impl Default for PackageMetadata {
    fn default() -> Self {
        Self::new()
    }
}
