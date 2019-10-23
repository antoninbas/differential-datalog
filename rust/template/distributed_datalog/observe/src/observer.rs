use std::collections::LinkedList;
use std::fmt::Debug;
use std::mem::replace;
use std::ops::Deref;
use std::ops::DerefMut;
use std::sync::Arc;
use std::sync::Mutex;

/// A boxed up `Observer`.
pub type ObserverBox<T, E> = Box<dyn Observer<T, E> + Send>;

/// A trait for objects that can observe an observable one.
pub trait Observer<T, E>: Debug + Send
where
    T: Send,
    E: Send,
{
    /// Action to perform before data starts coming in from the
    /// Observable.
    fn on_start(&mut self) -> Result<(), E>;

    /// Action to perform when a series of incoming data from the
    /// Observable is committed.
    fn on_commit(&mut self) -> Result<(), E>;

    /// Process a series of incoming items.
    fn on_updates<'a>(&mut self, updates: Box<dyn Iterator<Item = T> + 'a>) -> Result<(), E>;

    /// Action to perform when the `Observable` is about to shut down.
    ///
    /// This method is typically used to clean up any state associated
    /// with the `Observable`.
    fn on_completed(&mut self) -> Result<(), E>;
}

// We need a direct implementation of `Observer` for boxed up observers
// or Rust will complain in all sorts of undecipherable ways when we end
// up with nested `ObserverBox`s.
impl<T, E, O> Observer<T, E> for Box<O>
where
    T: Send,
    E: Send,
    O: Observer<T, E> + ?Sized,
{
    fn on_start(&mut self) -> Result<(), E> {
        self.deref_mut().on_start()
    }

    fn on_commit(&mut self) -> Result<(), E> {
        self.deref_mut().on_commit()
    }

    fn on_updates<'a>(&mut self, updates: Box<dyn Iterator<Item = T> + 'a>) -> Result<(), E> {
        self.deref_mut().on_updates(updates)
    }

    fn on_completed(&mut self) -> Result<(), E> {
        self.deref_mut().on_completed()
    }
}

/// Wrapper around an `Observer` that allows for it to be shared by
/// wrapping it into a combination of `Arc` & `Mutex`.
#[derive(Debug, Default)]
pub struct SharedObserver<O>(Arc<Mutex<O>>);

impl<O> SharedObserver<O> {
    /// Create a new `SharedObserver` object, automatically wrapping the
    /// provided `Observer` as necessary.
    pub fn new(observer: O) -> Self {
        SharedObserver(Arc::new(Mutex::new(observer)))
    }
}

impl<O> Clone for SharedObserver<O> {
    fn clone(&self) -> Self {
        SharedObserver(self.0.clone())
    }
}

impl<O> Deref for SharedObserver<O> {
    type Target = Arc<Mutex<O>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<O> DerefMut for SharedObserver<O> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<O, T, E> Observer<T, E> for SharedObserver<O>
where
    O: Observer<T, E>,
    T: Send,
    E: Send,
{
    fn on_start(&mut self) -> Result<(), E> {
        self.0.lock().unwrap().on_start()
    }

    fn on_commit(&mut self) -> Result<(), E> {
        self.0.lock().unwrap().on_commit()
    }

    fn on_updates<'a>(&mut self, updates: Box<dyn Iterator<Item = T> + 'a>) -> Result<(), E> {
        self.0.lock().unwrap().on_updates(updates)
    }

    fn on_completed(&mut self) -> Result<(), E> {
        self.0.lock().unwrap().on_completed()
    }
}

/// Wrapper around an `Observer` that stores updates and pushes them
/// forward only when an `on_commit` is received.
#[derive(Debug)]
pub struct CachingObserver<O, T> {
    /// The observer we ultimately push our data to when we received the
    /// `on_commit` event.
    observer: O,
    /// The data we accumulated so far.
    data: Option<LinkedList<Vec<T>>>,
}

impl<O, T> CachingObserver<O, T> {
    /// Create a new `CachingObserver` wrapping the provided observer.
    pub fn new(observer: O) -> Self {
        Self {
            observer,
            data: None,
        }
    }
}

impl<O, T, E> Observer<T, E> for CachingObserver<O, T>
where
    O: Observer<T, E>,
    T: Send + Debug,
    E: Send,
{
    fn on_start(&mut self) -> Result<(), E> {
        if self.data.is_none() {
            self.data = Some(LinkedList::new());
        } else {
            panic!("received multiple on_start events")
        }
        Ok(())
    }

    fn on_commit(&mut self) -> Result<(), E> {
        if let Some(ref mut data) = self.data.take() {
            self.observer.on_start()?;
            let updates_list = replace(data, LinkedList::new());
            for updates in updates_list.into_iter() {
                self.observer.on_updates(Box::new(updates.into_iter()))?;
            }
            self.observer.on_commit()?;
        } else {
            panic!("on_commit was not preceded by an on_start event")
        }
        Ok(())
    }

    fn on_updates<'a>(&mut self, updates: Box<dyn Iterator<Item = T> + 'a>) -> Result<(), E> {
        if let Some(ref mut data) = self.data {
            data.push_back(updates.collect());
        } else {
            panic!("on_updates was not preceded by an on_start event")
        }
        Ok(())
    }

    fn on_completed(&mut self) -> Result<(), E> {
        self.observer.on_completed()
    }
}

/// An `Observer` that wraps an inner, optional `Observer`. If the inner
/// one is unset all events will just be dropped.
#[derive(Debug)]
pub struct OptionalObserver<O>(Option<O>);

impl<O> OptionalObserver<O> {
    /// Create a new `OptionalObserver` object, automatically wrapping the
    /// provided `Observer` as necessary.
    pub fn new(observer: O) -> Self {
        Self(Some(observer))
    }

    /// Replace the existing `Observer` (if any) with the given optional
    /// one, returning the previous one.
    pub fn replace(&mut self, value: O) -> Option<O> {
        self.0.replace(value)
    }

    /// Take the inner observer, if any, replacing it with none.
    pub fn take(&mut self) -> Option<O> {
        self.0.take()
    }

    /// Check whether an actual observer is present or not.
    pub fn is_some(&self) -> bool {
        self.0.is_some()
    }
}

impl<O> Default for OptionalObserver<O> {
    fn default() -> Self {
        Self(None)
    }
}

impl<O, T, E> Observer<T, E> for OptionalObserver<O>
where
    O: Observer<T, E>,
    T: Send,
    E: Send,
{
    fn on_start(&mut self) -> Result<(), E> {
        self.0.as_mut().map_or(Ok(()), Observer::on_start)
    }

    fn on_commit(&mut self) -> Result<(), E> {
        self.0.as_mut().map_or(Ok(()), Observer::on_commit)
    }

    fn on_updates<'a>(&mut self, updates: Box<dyn Iterator<Item = T> + 'a>) -> Result<(), E> {
        self.0.as_mut().map_or(Ok(()), |o| o.on_updates(updates))
    }

    fn on_completed(&mut self) -> Result<(), E> {
        self.0.as_mut().map_or(Ok(()), Observer::on_completed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::test::MockObserver;

    /// Test caching of transactions via a `CachingObserver`.
    #[test]
    fn transaction_caching() {
        let mock = SharedObserver::new(MockObserver::new());
        let observer = &mut CachingObserver::new(mock.clone()) as &mut dyn Observer<_, ()>;

        assert_eq!(observer.on_start(), Ok(()));
        assert_eq!(mock.0.lock().unwrap().called_on_start, 0);

        assert_eq!(observer.on_updates(Box::new([1, 3, 2].iter())), Ok(()));
        assert_eq!(mock.0.lock().unwrap().called_on_updates, 0);

        assert_eq!(observer.on_updates(Box::new([6, 4, 5].iter())), Ok(()));
        assert_eq!(mock.0.lock().unwrap().called_on_updates, 0);

        assert_eq!(observer.on_commit(), Ok(()));
        assert_eq!(mock.0.lock().unwrap().called_on_start, 1);
        assert_eq!(mock.0.lock().unwrap().called_on_updates, 6);
        assert_eq!(mock.0.lock().unwrap().called_on_commit, 1);
    }

    /// Test the workings of an `OptionalObserver` with no actual
    /// observer present.
    #[test]
    fn no_optional_observer() {
        let observer = &mut OptionalObserver::<MockObserver>::default() as &mut dyn Observer<_, ()>;

        assert_eq!(observer.on_start(), Ok(()));
        assert_eq!(observer.on_updates(Box::new([1, 3, 2].iter())), Ok(()));
        assert_eq!(observer.on_commit(), Ok(()));
    }

    /// Test the workings of an `OptionalObserver` with an observer
    /// present.
    #[test]
    fn with_optional_observer() {
        let mock = SharedObserver::new(MockObserver::new());
        let observer = &mut OptionalObserver::new(mock.clone()) as &mut dyn Observer<_, ()>;

        assert_eq!(observer.on_start(), Ok(()));
        assert_eq!(mock.0.lock().unwrap().called_on_start, 1);

        assert_eq!(observer.on_updates(Box::new([1, 3, 2].iter())), Ok(()));
        assert_eq!(mock.0.lock().unwrap().called_on_updates, 3);

        assert_eq!(observer.on_commit(), Ok(()));
        assert_eq!(mock.0.lock().unwrap().called_on_commit, 1);
    }
}