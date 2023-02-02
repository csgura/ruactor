use std::{collections::HashMap, time::Duration};

use ruactor::{ask, Actor, ActorError, ActorRef, ActorSystem, PropClone, ReplyTo};

type Addr = String;

#[derive(Clone, Debug)]
pub struct Config {
    pub addr: Addr,
    pub reconnect_interval: Duration,
}

type NumConn = i32;
#[derive(Debug)]
enum ClientMessage {
    SendRequest {
        sender: ReplyTo<Result<Response, ActorError>>,
        method: String,
        addr: Addr,
        header: Header,
        body: Vec<u8>,
    },
    PrepareConnection {
        addr: Addr,
        num_conn: NumConn,
    },
}

#[derive(Debug)]
enum ChildMessage {
    SendRequest(
        ReplyTo<Result<Response, ActorError>>,
        String,
        Addr,
        Header,
        Vec<u8>,
    ),
    PrepareConnection(Addr, NumConn),
    _ConnectionDone(String),
    ConnectionError(ActorError),
}

#[derive(Clone)]
pub struct Client {
    actor_ref: ActorRef<ClientMessage>,
}

fn new_config() -> Config {
    Config {
        addr: "127.0.0.1:8080".into(),
        reconnect_interval: Duration::from_secs(1),
    }
}

#[derive(Clone)]
struct MainActor {
    cfg: Config,
}

#[derive(Clone)]
struct ChildActor {
    _cfg: Config,
    addr: Addr,
}

struct Connected {
    actor: ChildActor,
    _conn: String,
}

struct ConnectionFailed {
    actor: ChildActor,
    _err: ActorError,
}

impl Actor for ConnectionFailed {
    type Message = ChildMessage;

    fn on_message(
        &mut self,
        context: &mut ruactor::ActorContext<Self::Message>,
        message: Self::Message,
    ) {
        match message {
            ChildMessage::SendRequest(sender, _, _, _, _) => {
                println!("send error response!!");
                let _ = sender.send(Err(ActorError::SendError("send error".into())));
            }
            ChildMessage::PrepareConnection(_, _) => {
                context.transit(self.actor.clone());
            }
            ChildMessage::_ConnectionDone(_) => todo!(),
            ChildMessage::ConnectionError(_) => todo!(),
        }
    }

    fn on_enter(&mut self, context: &mut ruactor::ActorContext<Self::Message>) {
        println!("enter connection failed");
        context.start_single_timer(
            "reconnect",
            Duration::from_secs(10),
            //self.actor.cfg.reconnect_interval,
            ChildMessage::PrepareConnection(self.actor.addr.clone(), 2),
        );
    }

    fn on_exit(&mut self, _context: &mut ruactor::ActorContext<Self::Message>) {}

    fn on_system_message(
        &mut self,
        _context: &mut ruactor::ActorContext<Self::Message>,
        _message: ruactor::SystemMessage,
    ) {
    }
}

async fn send_request(
    sender: ReplyTo<Result<Response, ActorError>>,
    self_ref: ActorRef<ChildMessage>,
) {
    tokio::time::sleep(Duration::from_millis(10)).await;
    if true {
        let _ = sender.send(Ok(Response {
            _status: 200,
            _header: Default::default(),
            _body: Vec::new(),
        }));
    } else {
        let _ = sender.send(Err(ActorError::SendError("disconnected".into())));
        self_ref.tell(ChildMessage::ConnectionError(ActorError::SendError(
            "disconnected".into(),
        )))
    }
}

impl Actor for Connected {
    type Message = ChildMessage;

    fn on_message(
        &mut self,
        context: &mut ruactor::ActorContext<Self::Message>,
        message: Self::Message,
    ) {
        match message {
            ChildMessage::SendRequest(sender, method, addr, _, _) => {
                let self_ref = context.self_ref().clone();
                println!("send request {} , {}", method, addr);
                tokio::spawn(async move { send_request(sender, self_ref).await });
            }
            ChildMessage::PrepareConnection(_, _) => todo!(),
            ChildMessage::_ConnectionDone(_) => {}
            ChildMessage::ConnectionError(reason) => {
                context.transit(ConnectionFailed {
                    actor: self.actor.clone(),
                    _err: reason,
                });
            }
        }
    }

    fn on_enter(&mut self, _context: &mut ruactor::ActorContext<Self::Message>) {
        println!("become Connected");
    }

    fn on_exit(&mut self, _context: &mut ruactor::ActorContext<Self::Message>) {}

    fn on_system_message(
        &mut self,
        _context: &mut ruactor::ActorContext<Self::Message>,
        _message: ruactor::SystemMessage,
    ) {
    }
}
impl Actor for ChildActor {
    type Message = ChildMessage;

    fn on_message(
        &mut self,
        context: &mut ruactor::ActorContext<Self::Message>,
        message: Self::Message,
    ) {
        match message {
            ChildMessage::PrepareConnection(_, _) => todo!(),
            ChildMessage::_ConnectionDone(conn) => context.transit(Connected {
                actor: self.clone(),
                _conn: conn,
            }),
            ChildMessage::ConnectionError(err) => {
                context.transit(ConnectionFailed {
                    actor: self.clone(),
                    _err: err,
                });
            }
            _ => {
                context.stash(message);
            }
        }
    }

    fn on_enter(&mut self, context: &mut ruactor::ActorContext<Self::Message>) {
        println!("become ChildActor");
        let self_ref = context.self_ref();
        tokio::spawn(async move {
            println!("prepare connection");
            tokio::time::sleep(Duration::from_secs(1)).await;
            //self_ref.tell(ChildMessage::ConnectionDone("connected".into()));
            self_ref.tell(ChildMessage::ConnectionError(ActorError::SendError(
                "failed".into(),
            )));
        });
    }

    fn on_exit(&mut self, context: &mut ruactor::ActorContext<Self::Message>) {
        context.unstash_all();
    }

    fn on_system_message(
        &mut self,
        _context: &mut ruactor::ActorContext<Self::Message>,
        _message: ruactor::SystemMessage,
    ) {
    }
}
impl Actor for MainActor {
    type Message = ClientMessage;

    fn on_message(
        &mut self,
        context: &mut ruactor::ActorContext<Self::Message>,
        message: Self::Message,
    ) {
        match message {
            ClientMessage::SendRequest {
                sender,
                method,
                addr,
                header,
                body,
            } => {
                let child = context.get_or_create_child(
                    addr.clone(),
                    PropClone(ChildActor {
                        _cfg: self.cfg.clone(),
                        addr: addr.clone(),
                    }),
                );

                child.tell(ChildMessage::SendRequest(
                    sender, method, addr, header, body,
                ));
            }

            ClientMessage::PrepareConnection { addr, num_conn } => {
                let child = context.get_or_create_child(
                    addr.clone(),
                    PropClone(ChildActor {
                        _cfg: self.cfg.clone(),
                        addr: addr.clone(),
                    }),
                );

                child.tell(ChildMessage::PrepareConnection(addr, num_conn));
            }
        }
    }
}

#[derive(Debug)]
pub struct Response {
    _status: i16,
    _header: Header,
    _body: Vec<u8>,
}

type Header = HashMap<String, Vec<String>>;
impl Client {
    pub fn new(asys: ActorSystem) -> Self {
        let cfg = new_config();
        let actor_ref = asys
            .create_actor("sbigw-main-actor", PropClone(MainActor { cfg }))
            .expect("actor creation failed");

        Client { actor_ref }
    }

    pub async fn send_request(
        &self,
        method: String,
        addr: String,
        header: Header,
        body: Vec<u8>,
    ) -> Result<Response, ActorError> {
        let res = ask!(
            self.actor_ref,
            ClientMessage::SendRequest {
                sender: _,
                method,
                addr,
                header,
                body
            },
            Duration::from_secs(10)
        )??;
        Ok(res)
    }
}

#[tokio::main]
async fn main() {
    let asys = ActorSystem::new("hello");
    let cli = Client::new(asys);

    let addr = String::from("hello");
    let num_conn = 23;

    let _pc = ClientMessage::PrepareConnection {
        addr: addr,
        num_conn,
    };

    let _ = cli
        .send_request(
            "POST".into(),
            "localhost".into(),
            Default::default(),
            Default::default(),
        )
        .await;
    loop {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
