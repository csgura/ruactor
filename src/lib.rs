mod actor;
mod path;
mod system;

pub use actor::Actor;
pub use actor::ActorContext;
pub use actor::ActorRef;
pub use actor::SystemMessage;
pub use system::ActorSystem;
pub use system::Prop;

#[macro_export]
macro_rules! ask {
    ($enum_name:ident::$variant:ident( _, $($e:expr),* )  ) => {
        {
            let (reply_to, rx) = tokio::sync::oneshot::channel();
            let _m = $enum_name::$variant( reply_to, $($e,)*);
            rx.await
        }
    };
    ($enum_name:ident::$variant:ident( $($e:expr),* , _ )  ) => {
        {
            let (reply_to, rx) = tokio::sync::oneshot::channel();
            let _m = $enum_name::$variant(  $($e,)*  reply_to,);
            rx.await
        }
    };
    ($enum_name:ident::$variant:ident( $($e1:expr),* , _ , $($e2:expr),*)  ) => {
        {
            let (reply_to, rx) = tokio::sync::oneshot::channel();
            let _m = $enum_name::$variant(  $($e1,)*  reply_to, $($e2,)*);
            rx.await
        }
    };
}

#[macro_export]
macro_rules! reply_to {
    ($e:expr, $e2:expr) => {
        if let Some(ch) = std::mem::replace(&mut $e, None) {
            ch.send($e2)
        } else {
            Err($e2)
        }
    };
}
