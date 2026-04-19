use blockifier::state::state_api::StateReader;

pub trait ReexecutionStateReader: StateReader {}

pub trait ConsecutiveReexecutionStateReaders<S: StateReader + Send + Sync + 'static>: Sized {}
