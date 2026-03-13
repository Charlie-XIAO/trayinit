use std::marker::PhantomData;

use crate::Tray;

pub(crate) struct PlatformHandle<T: Tray> {
    _phantom: PhantomData<T>,
}
