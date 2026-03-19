use crate::message::ConnectProto;
use buffa::Message;
use bytes::Buf;
use std::marker::PhantomData;
use tonic::Status;
use tonic::codec::{BufferSettings, Codec, DecodeBuf, Decoder, EncodeBuf, Encoder};

/// A tonic codec backed by `buffa::Message`.
#[derive(Debug, Clone)]
pub struct BuffaCodec<T, U> {
    _marker: PhantomData<(T, U)>,
}

impl<T, U> BuffaCodec<T, U> {
    pub fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }

    pub fn raw_encoder(buffer_settings: BufferSettings) -> BuffaEncoder<T> {
        BuffaEncoder {
            _marker: PhantomData,
            buffer_settings,
        }
    }

    pub fn raw_decoder(buffer_settings: BufferSettings) -> BuffaDecoder<U> {
        BuffaDecoder {
            _marker: PhantomData,
            buffer_settings,
        }
    }
}

impl<T, U> Default for BuffaCodec<T, U> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, U> Codec for BuffaCodec<T, U>
where
    T: Message + Send + 'static,
    U: ConnectProto + Send + 'static,
{
    type Encode = T;
    type Decode = U;
    type Encoder = BuffaEncoder<T>;
    type Decoder = BuffaDecoder<U>;

    fn encoder(&mut self) -> Self::Encoder {
        BuffaEncoder {
            _marker: PhantomData,
            buffer_settings: BufferSettings::default(),
        }
    }

    fn decoder(&mut self) -> Self::Decoder {
        BuffaDecoder {
            _marker: PhantomData,
            buffer_settings: BufferSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct BuffaEncoder<T> {
    _marker: PhantomData<T>,
    buffer_settings: BufferSettings,
}

impl<T> Encoder for BuffaEncoder<T>
where
    T: Message,
{
    type Item = T;
    type Error = Status;

    fn encode(&mut self, item: Self::Item, buf: &mut EncodeBuf<'_>) -> Result<(), Self::Error> {
        item.encode(buf);
        Ok(())
    }

    fn buffer_settings(&self) -> BufferSettings {
        self.buffer_settings
    }
}

#[derive(Debug, Clone, Default)]
pub struct BuffaDecoder<U> {
    _marker: PhantomData<U>,
    buffer_settings: BufferSettings,
}

impl<U> Decoder for BuffaDecoder<U>
where
    U: ConnectProto,
{
    type Item = U;
    type Error = Status;

    fn decode(&mut self, buf: &mut DecodeBuf<'_>) -> Result<Option<Self::Item>, Self::Error> {
        U::decode_proto(buf.copy_to_bytes(buf.remaining()))
            .map(Some)
            .map_err(|error| Status::internal(error.message().unwrap_or("decode failed")))
    }

    fn buffer_settings(&self) -> BufferSettings {
        self.buffer_settings
    }
}
