mod actor;
mod path;
mod system;

pub use actor::Actor;
pub use actor::ActorContext;
pub use actor::ActorRef;
pub use actor::AutoMessage;
pub use actor::AutoRef;
pub use actor::SystemMessage;
pub use path::ActorPath;
pub use system::ActorSystem;

pub use system::props_from_clone;
pub use system::props_from_func;
pub use system::Props;
pub use system::PropsOption;

#[deprecated]
pub use system::PropClone;

#[deprecated]
pub use system::PropFunc;

use thiserror::Error;
use tokio::sync::oneshot;
use tokio::time::error::Elapsed;

#[derive(Error, Debug)]
pub enum ActorError {
    #[error("Actor exists")]
    Exists(ActorPath),

    #[error("Actor creation failed")]
    CreateError(String),

    #[error("Sending message failed")]
    SendError(String),

    #[error("Actor runtime error")]
    RuntimeError(anyhow::Error),

    #[error("Actor Recv Error")]
    RecvError(#[from] oneshot::error::RecvError),

    #[error("Actor Timeout Error")]
    TimeoutError(#[from] Elapsed),
}

impl ActorError {
    pub fn new<E>(error: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::RuntimeError(anyhow::Error::new(error))
    }
}

#[macro_export]
macro_rules! ask {
    (@tell: $actor_ref:expr, $m:expr, $rx:expr, $tmout:expr) => {
        {
            let send_ok = ($actor_ref).try_tell($m);
            match send_ok {
                Ok(_) => {
                    let recv = tokio::time::timeout($tmout, $rx);
                    match recv.await {
                        Ok(Ok(v)) => Ok(v),
                        Ok(Err(err)) => Err($crate::ActorError::from(err)),
                        Err(err) => Err($crate::ActorError::from(err)),
                    }
                },
                Err(_) => {
                    Err($crate::ActorError::SendError(format!("actor {} mailbox full", $actor_ref)))
                }
            }

        }
    };

    ($actor_ref:expr, $enum_name:ident::$variant:ident( _ ) , $tmout:expr ) => {
        {
            let (reply_to, rx) = tokio::sync::oneshot::channel();
            let m = $enum_name::$variant( reply_to );
			ask!(@tell: $actor_ref , m , rx, $tmout)
        }
    };

    ($actor_ref:expr, $enum_name:ident::$variant:ident( _, $($e:expr),* ) , $tmout:expr ) => {
        {
            let (reply_to, rx) = tokio::sync::oneshot::channel();
            let m = $enum_name::$variant( reply_to, $($e,)*);
			ask!(@tell: $actor_ref , m , rx, $tmout)
        }
    };
    ($actor_ref:expr, $enum_name:ident::$variant:ident( $($e:expr),* , _ ) , $tmout:expr ) => {
        {
            let (reply_to, rx) = tokio::sync::oneshot::channel();
            let m = $enum_name::$variant(  $($e,)*  reply_to);
			ask!(@tell: $actor_ref , m , rx, $tmout)
        }
    };
    ($actor_ref:expr, $enum_name:ident::$variant:ident( $($e1:expr),* , _ , $($e2:expr),*) , $tmout:expr ) => {
        {
            let (reply_to, rx) = tokio::sync::oneshot::channel();
            let m = $enum_name::$variant(  $($e1,)*  reply_to, $($e2,)*);
			ask!(@tell: $actor_ref , m , rx, $tmout)
        }
    };

    ($actor_ref:expr, $enum_name:ident::$variant:ident{ $reply_id:ident : _ , $($tail:tt)* } , $tmout:expr ) => {
        {
            let (reply_to, rx) = tokio::sync::oneshot::channel();
            let m = $enum_name::$variant{ $reply_id : reply_to, $($tail)*};
            ask!(@tell: $actor_ref , m , rx, $tmout)
        }
    };
}

pub type ReplyTo<T> = tokio::sync::oneshot::Sender<T>;

#[macro_export]
macro_rules! reply_to {
    ($e:expr, $e2:expr) => {{
        if let Some(ch) = std::mem::replace(&mut $e, None) {
            ch.send($e2)
        } else {
            Err($e2)
        }
    }};
}

pub async fn benchmark_actor_loop<T: 'static + Send>(actor_ref: ActorRef<T>, bulk: Vec<T>) {
    bulk.into_iter().for_each(|m| {
        let _ = actor_ref.mbox.message_queue.push(actor::Message::User(m));
    });

    let mut dp = actor_ref.mbox.dispatcher.lock().await;
    dp.actor_loop(actor_ref.clone());
}
