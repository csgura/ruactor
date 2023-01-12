mod actor;
mod bus;
mod system;

use std::marker::PhantomData;

use actor::ReceiveMethod;
pub use actor::{
    supervision::{RetryStrategy, SupervisionStrategy},
    Actor, ActorContext, ActorError, ActorPath, ActorRef, Handler, Message, Receive, Request,
};
pub mod supervision {

    pub use crate::actor::supervision::{
        ExponentialBackoffStrategy, FixedIntervalStrategy, NoIntervalStrategy,
    };
}
pub use bus::EventReceiver;
pub use system::ActorSystem;

pub use async_trait::async_trait;

pub use actor::create_receive;
