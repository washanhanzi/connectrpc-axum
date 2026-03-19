use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

use buffa::{DecodeError, DefaultInstance, Message, UnknownFields};

#[derive(Debug)]
pub(crate) struct TestCachedSize {
    size: AtomicU32,
}

impl TestCachedSize {
    const fn new() -> Self {
        Self {
            size: AtomicU32::new(0),
        }
    }

    fn get(&self) -> u32 {
        self.size.load(Ordering::Relaxed)
    }

    fn set(&self, size: u32) {
        self.size.store(size, Ordering::Relaxed);
    }
}

impl Default for TestCachedSize {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for TestCachedSize {
    fn clone(&self) -> Self {
        Self::new()
    }
}

impl PartialEq for TestCachedSize {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct TestMessage {
    pub(crate) value: String,
    #[serde(skip)]
    pub(crate) __buffa_unknown_fields: UnknownFields,
    #[serde(skip)]
    pub(crate) __buffa_cached_size: TestCachedSize,
}

unsafe impl DefaultInstance for TestMessage {
    fn default_instance() -> &'static Self {
        static VALUE: OnceLock<TestMessage> = OnceLock::new();
        VALUE.get_or_init(TestMessage::default)
    }
}

impl Message for TestMessage {
    fn compute_size(&self) -> u32 {
        let mut size = 0u32;
        if !self.value.is_empty() {
            size += 1 + buffa::types::string_encoded_len(&self.value) as u32;
        }
        size += self.__buffa_unknown_fields.encoded_len() as u32;
        self.__buffa_cached_size.set(size);
        size
    }

    fn write_to(&self, buf: &mut impl bytes::BufMut) {
        if !self.value.is_empty() {
            buffa::encoding::Tag::new(1, buffa::encoding::WireType::LengthDelimited).encode(buf);
            buffa::types::encode_string(&self.value, buf);
        }
        self.__buffa_unknown_fields.write_to(buf);
    }

    fn merge_field(
        &mut self,
        tag: buffa::encoding::Tag,
        buf: &mut impl bytes::Buf,
        depth: u32,
    ) -> Result<(), DecodeError> {
        match tag.field_number() {
            1 => {
                if tag.wire_type() != buffa::encoding::WireType::LengthDelimited {
                    return Err(DecodeError::WireTypeMismatch {
                        field_number: 1,
                        expected: buffa::encoding::WireType::LengthDelimited as u8,
                        actual: tag.wire_type() as u8,
                    });
                }
                buffa::types::merge_string(&mut self.value, buf)?;
            }
            _ => {
                self.__buffa_unknown_fields
                    .push(buffa::encoding::decode_unknown_field(tag, buf, depth)?);
            }
        }
        Ok(())
    }

    fn cached_size(&self) -> u32 {
        self.__buffa_cached_size.get()
    }

    fn clear(&mut self) {
        self.value.clear();
        self.__buffa_unknown_fields.clear();
        self.__buffa_cached_size.set(0);
    }
}
