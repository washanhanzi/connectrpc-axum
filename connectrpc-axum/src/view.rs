use buffa::bytes::Bytes;
use std::fmt;
use std::ops::Deref;

/// Associates an owned Buffa message with its generated borrowed view type.
pub trait HasView: buffa::Message {
    type View<'a>: buffa::MessageView<'a, Owned = Self>
    where
        Self: 'a;
}

/// A self-contained zero-copy view backed by owned protobuf bytes.
pub struct View<T: HasView> {
    inner: buffa::OwnedView<T::View<'static>>,
}

impl<T: HasView> View<T> {
    /// Decode a view directly from protobuf bytes.
    pub fn decode(bytes: Bytes) -> Result<Self, buffa::DecodeError> {
        buffa::OwnedView::decode(bytes).map(Self::from_raw)
    }

    /// Decode a view from an owned message by round-tripping through protobuf.
    pub fn from_owned(message: &T) -> Result<Self, buffa::DecodeError> {
        buffa::OwnedView::from_owned(message).map(Self::from_raw)
    }

    /// Wrap a raw Buffa owned view.
    pub fn from_raw(inner: buffa::OwnedView<T::View<'static>>) -> Self {
        Self { inner }
    }

    /// Borrow the underlying generated view.
    pub fn as_view(&self) -> &T::View<'static> {
        &self.inner
    }

    /// Convert this view into the owned message.
    pub fn into_owned(self) -> T {
        self.inner.to_owned_message()
    }

    /// Consume the wrapper and return the raw Buffa owned view.
    pub fn into_raw(self) -> buffa::OwnedView<T::View<'static>> {
        self.inner
    }

    /// Borrow the backing protobuf bytes.
    pub fn bytes(&self) -> &Bytes {
        self.inner.bytes()
    }
}

impl<T: HasView> Deref for View<T> {
    type Target = T::View<'static>;

    fn deref(&self) -> &Self::Target {
        self.as_view()
    }
}

impl<T> Clone for View<T>
where
    T: HasView,
    T::View<'static>: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<T> fmt::Debug for View<T>
where
    T: HasView,
    T::View<'static>: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.inner.fmt(f)
    }
}

impl<T> PartialEq for View<T>
where
    T: HasView,
    T::View<'static>: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

impl<T> Eq for View<T>
where
    T: HasView,
    T::View<'static>: Eq,
{
}
