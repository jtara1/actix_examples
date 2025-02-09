use log::{debug, info};

use actix::fut;
use actix::prelude::*;
use actix_broker::BrokerIssue;
use actix_web_actors::ws;

use crate::message::{
    ChatMessage, JoinRoom, LeaveRoom, ListClients, ListRooms, SendMessage,
};
use crate::server::WsChatServer;

#[derive(Default)]
pub struct WsChatSession {
    client_id: usize,
    room_name: String,
    client_name: Option<String>,
}

impl WsChatSession {
    /// Getter for self.name, the client's name for this session
    pub fn client_name(&self) -> String {
        self.client_name
            .as_ref()
            .unwrap_or(&String::from("anon"))
            .clone()
    }

    pub fn join_room(&mut self, room_name: &str, ctx: &mut ws::WebsocketContext<Self>) {
        let room_name = room_name.to_owned();

        // Then send a join message for the new room
        let join_msg = JoinRoom(
            room_name.to_owned(),
            self.client_name(),
            ctx.address().recipient(),
        );

        WsChatServer::from_registry()
            .send(join_msg)
            .into_actor(self)
            .then(|id, act, _ctx| {
                if let Ok(id) = id {
                    act.client_id = id;
                    act.room_name = room_name;
                }

                fut::ready(())
            })
            .wait(ctx);
    }

    pub fn list_rooms(&mut self, ctx: &mut ws::WebsocketContext<Self>) {
        WsChatServer::from_registry()
            .send(ListRooms)
            .into_actor(self)
            .then(|result, _, ctx| {
                if let Ok(rooms) = result {
                    for room in rooms {
                        ctx.text(room);
                    }
                }

                fut::ready(())
            })
            .wait(ctx);
    }

    pub fn list_clients(&mut self, ctx: &mut ws::WebsocketContext<Self>) {
        WsChatServer::from_registry()
            .send(ListClients(self.room_name.clone()))
            .into_actor(self)
            .then(|result, _, ctx| {
                if let Ok(clients) = result {
                    for client in clients {
                        ctx.text(client);
                    }
                }

                fut::ready(())
            })
            .wait(ctx);
    }

    pub fn send_msg(&self, msg: &str) {
        let content = format!("{}: {}", self.client_name(), msg);

        let msg = SendMessage(self.room_name.clone(), self.client_id, content);

        // issue_async comes from having the `BrokerIssue` trait in scope.
        self.issue_system_async(msg);
    }

    pub fn who_am_i(&self, ctx: &mut ws::WebsocketContext<Self>) {
        let msg = format!(
            "name: {}, client_id: {} in room_name: {}",
            self.client_name(),
            self.client_id,
            self.room_name
        );
        ctx.text(msg);
    }
}

impl Actor for WsChatSession {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.join_room("Main", ctx);
    }

    fn stopped(&mut self, ctx: &mut Self::Context) {
        // send a leave message for the current room
        let leave_msg = LeaveRoom(self.room_name.clone(), self.client_id);

        // issue_sync comes from having the `BrokerIssue` trait in scope.
        self.issue_system_sync(leave_msg, ctx);

        info!(
            "WsChatSession closed for {}({}) in room {}",
            self.client_name(),
            self.client_id,
            self.room_name
        );
    }
}

impl Handler<ChatMessage> for WsChatSession {
    type Result = ();

    fn handle(&mut self, msg: ChatMessage, ctx: &mut Self::Context) {
        ctx.text(msg.0);
    }
}

impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WsChatSession {
    fn handle(
        &mut self,
        msg: Result<ws::Message, ws::ProtocolError>,
        ctx: &mut Self::Context,
    ) {
        let msg = match msg {
            Err(_) => {
                ctx.stop();
                return;
            }
            Ok(msg) => msg,
        };

        debug!(
            "WsChatSession::handle() - message: {:?} from: {}",
            msg,
            self.client_name()
        );

        match msg {
            ws::Message::Text(text) => {
                let msg = text.trim();

                if msg.starts_with('/') {
                    let mut command = msg.splitn(2, ' ');

                    match command.next() {
                        Some("/list") => self.list_rooms(ctx),

                        Some("/join") => {
                            if let Some(room_name) = command.next() {
                                self.join_room(room_name, ctx);
                            } else {
                                ctx.text("!!! room name is required");
                            }
                        }

                        Some("/name") => {
                            if let Some(name) = command.next() {
                                self.client_name = Some(name.to_owned());
                                ctx.text(format!("name changed to: {}", name));
                            } else {
                                ctx.text("!!! name is required");
                            }
                        }

                        Some("/list-clients") => self.list_clients(ctx),

                        Some("/whoami") => self.who_am_i(ctx),

                        _ => ctx.text(format!("!!! unknown command: {:?}", msg)),
                    }

                    return;
                }
                self.send_msg(msg);
            }
            ws::Message::Close(reason) => {
                ctx.close(reason);
                ctx.stop();
            }
            _ => {}
        }
    }
}
