use std::marker::PhantomData;

use crate::{Builder, ClosedError, Error, Handle, Result, Tray};

#[derive(Debug)]
pub struct PlatformHandle<T: Tray> {
    _phantom: PhantomData<T>,
}

impl<T: Tray> Clone for PlatformHandle<T> {
    fn clone(&self) -> Self {
        Self {
            _phantom: PhantomData,
        }
    }
}

impl<T: Tray> PlatformHandle<T> {
    pub fn update<R>(&self, f: impl FnOnce(&mut T) -> R) -> Result<R, ClosedError> {
        let _ = f;
        unimplemented!()
    }

    pub fn refresh(&self) -> Result<(), ClosedError> {
        unimplemented!()
    }

    pub fn shutdown(&self) -> Result<()> {
        unimplemented!()
    }

    pub fn is_closed(&self) -> bool {
        unimplemented!()
    }
}

pub fn spawn<T: Tray>(builder: Builder<T>) -> Result<Handle<T>>
where
    T::Message: Clone,
{
    let _ = builder;
    Err(Error::NotImplemented)
}

pub fn attach<T: Tray>(builder: Builder<T>) -> Result<Handle<T>>
where
    T::Message: Clone,
{
    let _ = builder;
    Err(Error::NotImplemented)
}

pub fn run<T: Tray>(builder: Builder<T>) -> Result<()>
where
    T::Message: Clone,
{
    let _ = builder;
    Err(Error::NotImplemented)
}
