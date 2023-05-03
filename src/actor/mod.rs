use std::{
    any::Any,
    borrow::Cow,
    collections::BinaryHeap,
    fmt::{Debug, Display},
    marker::PhantomData,
    sync::Arc,
    time::{Duration, Instant},
};

mod context;
mod dispatcher;
mod mailbox;

use crate::{
    path::ActorPath,
    system::{PropDyn, Props},
    ReplyTo,
};

#[async_trait]
pub(crate) trait InternalActorRef: 'static + Send + Sync + Debug {
    fn stop(&self);
    async fn graceful_stop(&self);
    async fn wait_stop(&self);
    async fn watch(&self);
}

pub struct ActorRef<T: 'static + Send> {
    pub(crate) mbox: Arc<Mailbox<T>>,
    pub(crate) path: Arc<ActorPath>,
}

#[async_trait]
impl<T: 'static + Send> InternalActorRef for ActorRef<T> {
    fn stop(&self) {
        self.send_internal_message(InternalMessage::Terminate);
    }

    async fn graceful_stop(&self) {
        self.send_auto_message(AutoMessage::PoisonPill);
        self.watch().await;
    }

    async fn wait_stop(&self) {
        if self.mbox.option.graceful_stop() {
            self.graceful_stop().await;
        } else {
            self.stop();
            self.watch().await;
        }
    }

    async fn watch(&self) {
        if self.mbox.is_terminated() {
            return;
        }

        let ch = tokio::sync::oneshot::channel();
        self.mbox
            .send_internal(self.clone(), InternalMessage::Watch(ch.0));
        let _ = ch.1.await;
    }
}

#[async_trait]
pub trait AutoRef {
    fn send_auto_message(&self, message: AutoMessage);
}

impl<T: 'static + Send> AutoRef for ActorRef<T> {
    fn send_auto_message(&self, message: AutoMessage) {
        self.send(Message::AutoMessage(message))
    }
}

pub(crate) trait ParentRef: 'static + Send {
    fn send_internal_message(&self, message: InternalMessage);
}

impl<T: 'static + Send> ParentRef for ActorRef<T> {
    fn send_internal_message(&self, message: InternalMessage) {
        self.mbox.send_internal(self.clone(), message)
    }
}

impl<T: 'static + Send> Display for ActorRef<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.path())
    }
}

impl<T: 'static + Send> Debug for ActorRef<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActorRef")
            .field("path", &self.path)
            .finish()
    }
}

use async_trait::async_trait;
use crossbeam::queue::SegQueue;
pub use dispatcher::Dispatcher;
pub use mailbox::Mailbox;

impl<T: 'static + Send> ActorRef<T> {
    pub fn new(path: ActorPath, mbox: Arc<Mailbox<T>>) -> ActorRef<T> {
        ActorRef {
            mbox: mbox,
            path: Arc::new(path),
        }
    }

    pub fn path(&self) -> &ActorPath {
        &self.path
    }
}

impl<T: 'static + Send> Clone for ActorRef<T> {
    fn clone(&self) -> Self {
        Self {
            mbox: self.mbox.clone(),
            path: self.path.clone(),
        }
    }
}

impl<T: 'static + Send> ActorRef<T> {
    pub fn tell(&self, msg: T) {
        self.mbox.send(self.clone(), Message::User(msg));
    }

    pub fn try_tell(&self, msg: T) -> Result<(), T> {
        let res = self.mbox.try_send(self.clone(), Message::User(msg));
        res.map_err(|err| match err {
            Message::User(msg) => msg,
            _ => panic!("not possible"),
        })
    }

    pub(crate) fn send(&self, msg: Message<T>) {
        self.mbox.send(self.clone(), msg);
    }
}

pub use context::ActorContext;
pub use dispatcher::Timer;

#[derive(Debug)]
pub enum SystemMessage {
    ReceiveTimeout,
}

#[derive(Debug)]
pub enum AutoMessage {
    PoisonPill,
}

#[derive(Debug)]
pub(crate) enum Message<T: 'static + Send> {
    User(T),
    Timer(Cow<'static, str>, u32, T),
    ReceiveTimeout(Instant),
    AutoMessage(AutoMessage),
}

impl<T: 'static + Send> Message<T> {
    pub(crate) fn kind(&self) -> &str {
        match self {
            Message::User(_) => "user message",
            Message::Timer(_, _, _) => "timer message",
            Message::ReceiveTimeout(_) => "receive timeout",
            Message::AutoMessage(_) => "auto message",
        }
    }
}

#[derive(Debug)]
pub(crate) enum InternalMessage {
    Created,
    ChildTerminate(Arc<ActorPath>),
    Terminate,
    Watch(ReplyTo<()>),
}

#[allow(unused_variables)]
#[async_trait]
pub trait Actor: Send + 'static {
    type Message: 'static + Send;

    async fn on_message_async(
        &mut self,
        context: &mut ActorContext<Self::Message>,
        message: Self::Message,
    ) {
        self.on_message(context, message)
    }

    fn on_enter(&mut self, context: &mut ActorContext<Self::Message>) {}

    fn on_exit(&mut self, context: &mut ActorContext<Self::Message>) {}

    fn on_message(&mut self, context: &mut ActorContext<Self::Message>, message: Self::Message);

    fn on_system_message(
        &mut self,
        context: &mut ActorContext<Self::Message>,
        message: SystemMessage,
    ) {
        match message {
            SystemMessage::ReceiveTimeout => context.stop_self(),
        }
    }
}

struct PropWrap<A: Actor, P: Props<A>> {
    prop: P,
    phantom: PhantomData<A>,
}

impl<A: Actor, P: Props<A>> PropDyn<A::Message> for PropWrap<A, P> {
    fn create(&self) -> Box<dyn Actor<Message = A::Message>> {
        Box::new(self.prop.create())
    }

    fn option(&self) -> crate::PropsOption {
        self.prop.option()
    }
}

pub(crate) struct ChildContainer {
    pub(crate) actor_ref: Box<dyn Any + Send + Sync + 'static>,
    pub(crate) stop_ref: Box<dyn InternalActorRef>,
}

pub struct SchedulerTask {
    event_time: Instant,
    action: Box<dyn FnOnce() + Send>,
}

impl PartialEq for SchedulerTask {
    fn eq(&self, other: &Self) -> bool {
        self.event_time == other.event_time
    }
}

impl Eq for SchedulerTask {}

impl PartialOrd for SchedulerTask {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.event_time.cmp(&other.event_time).reverse())
    }
}

impl Ord for SchedulerTask {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.event_time.cmp(&other.event_time).reverse()
    }
}

enum SchedulerMessage {
    Close,
    Task(SchedulerTask),
}

#[derive(Clone)]
pub struct Scheduler {
    queue: Arc<SegQueue<SchedulerMessage>>,
}

impl Drop for Scheduler {
    fn drop(&mut self) {
        self.queue.push(SchedulerMessage::Close)
    }
}
impl Scheduler {
    pub(crate) fn after_func(&self, d: Duration, f: impl FnOnce() + 'static + Send) {
        self.queue.push(SchedulerMessage::Task(SchedulerTask {
            event_time: Instant::now() + d,
            action: Box::new(f),
        }))
    }

    pub(crate) fn new() -> Self {
        Scheduler {
            queue: Default::default(),
        }
    }

    pub(crate) fn run(&self, name: &str) {
        let queue = self.queue.clone();

        let res = std::thread::Builder::new()
            .name(format!("{}-scheduler", name))
            .spawn(move || {
                let mut bheap: BinaryHeap<SchedulerTask> = BinaryHeap::new();

                loop {
                    while let Some(msg) = queue.pop() {
                        match msg {
                            SchedulerMessage::Close => {
                                return;
                            }
                            SchedulerMessage::Task(task) => bheap.push(task),
                        }
                    }

                    let now = Instant::now();

                    while let Some(task) = bheap.pop() {
                        if task.event_time < now {
                            (task.action)()
                        } else {
                            bheap.push(task);
                            break;
                        }
                    }

                    if queue.len() == 0 {
                        std::thread::sleep(Duration::from_millis(1));
                    }
                }
            });
        if let Err(err) = res {
            panic!("can't start scheduler thread : {}", err);
        }
    }
}
