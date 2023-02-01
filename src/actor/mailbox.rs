use std::{
    marker::PhantomData,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
    time::Instant,
};

use tokio::sync::Mutex;

use crate::{Actor, Prop};

use super::{context::ActorCell, ActorRef, Dispatcher, Message, ParentRef, PropWrap};

pub struct Mailbox<T: 'static + Send> {
    pub(crate) ch: tokio::sync::mpsc::UnboundedSender<Message<T>>,
    pub(crate) num_msg: Arc<AtomicU32>,
    pub(crate) status: Arc<AtomicBool>,
    pub(crate) cell: Arc<Mutex<Dispatcher<T>>>,
    pub(crate) handle: tokio::runtime::Handle,
}

impl<T: 'static + Send> Clone for Mailbox<T> {
    fn clone(&self) -> Self {
        Self {
            ch: self.ch.clone(),
            num_msg: self.num_msg.clone(),
            status: self.status.clone(),
            cell: self.cell.clone(),
            handle: self.handle.clone(),
        }
    }
}

impl<T: 'static + Send> Mailbox<T> {
    pub(crate) fn new<P, A>(p: P, parent: Option<Box<dyn ParentRef>>) -> Mailbox<T>
    where
        P: Prop<A>,
        A: Actor<Message = T>,
    {
        let pdyn = PropWrap {
            prop: p,
            phantom: PhantomData,
        };

        let ch = tokio::sync::mpsc::unbounded_channel::<Message<T>>();

        let cell = Dispatcher {
            actor: None,
            ch: ch.1,
            prop: Box::new(pdyn),
            last_message_timestamp: Instant::now(),
            cell: ActorCell::new(parent),
        };

        let mbox = Mailbox {
            cell: Arc::new(Mutex::new(cell)),
            ch: ch.0,
            num_msg: Arc::new(0.into()),
            status: Arc::new(false.into()),
            handle: tokio::runtime::Handle::current(),
        };
        mbox
    }

    //#[async_recursion::async_recursion]
    pub(crate) async fn receive(&self, self_ref: ActorRef<T>) {
        let mut owned = false;

        if let Ok(mut cell) = self.cell.try_lock() {
            //println!("start receive loop");

            owned = true;
            self.status.store(true, Ordering::SeqCst);
            cell.actor_loop(self_ref.clone()).await;
            self.status.store(false, Ordering::SeqCst);
            //println!("end receive loop");
        } else {
            //println!("lock failed");
        }

        if owned {
            let num_msg = self.num_msg.load(Ordering::SeqCst);
            if num_msg > 0 {
                //println!("num msg = {}", num_msg);
                //self.receive().await;
                self.schedule(self_ref.clone());
            }
        }
    }

    pub(crate) fn schedule(&self, self_ref: ActorRef<T>) {
        if !self.status.load(Ordering::SeqCst) {
            //println!("schedule");
            let cl: Mailbox<T> = self.clone();

            self.handle.spawn(async move {
                cl.receive(self_ref.clone()).await
                //cl.status.store(true, std::sync::atomic::Ordering::SeqCst);
                //actor_loop( &mut cell ).await;
            });
        } else {
            //println!("not schedule");
        }
    }

    pub(crate) fn send(&self, self_ref: ActorRef<T>, msg: Message<T>) {
        let ch = self.ch.clone();
        match ch.send(msg) {
            Ok(_) => {
                self.num_msg.fetch_add(1, Ordering::SeqCst);
                self.schedule(self_ref);
            }
            Err(_) => {
                log::warn!("dead letter message to {} ", self_ref);
            }
        }
    }
}
