//! A little-endian body builder for server packets.

/// Appends wire values to a packet body.
#[derive(Default)]
pub struct Body(pub Vec<u8>);

impl Body {
    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn f32(&mut self, v: f32) -> &mut Self {
        self.0.extend_from_slice(&v.to_le_bytes());
        self
    }
    pub fn bytes(&mut self, v: &[u8]) -> &mut Self {
        self.0.extend_from_slice(v);
        self
    }
    /// A NUL-terminated string.
    pub fn cstring(&mut self, v: &str) -> &mut Self {
        self.0.extend_from_slice(v.as_bytes());
        self.0.push(0);
        self
    }
    /// A packed guid: a mask of the non-zero bytes, then those bytes low to high.
    pub fn packed_guid(&mut self, guid: u64) -> &mut Self {
        benilla_protocol::wire::write_packed_guid(guid, &mut self.0).expect("Vec write");
        self
    }
    pub fn done(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.0)
    }
}
