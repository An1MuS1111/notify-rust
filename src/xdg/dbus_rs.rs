use dbus::{
    arg::messageitem::{MessageItem, MessageItemArray},
    ffidisp::{BusType, Connection, ConnectionItem},
    Message,
};

use super::{bus::NotificationBus, ActionResponse, ActionResponseHandler, NOTIFICATION_INTERFACE};

use crate::{
    error::*,
    hints::message::HintMessage,
    notification::Notification,
    xdg::{ServerInformation, NOTIFICATION_OBJECTPATH},
};

pub mod bus;

mod handle;
pub use handle::DbusNotificationHandle;

pub fn send_notification_via_connection(
    notification: &Notification,
    id: u32,
    connection: &Connection,
) -> Result<u32> {
    send_notification_via_connection_at_bus(notification, id, connection, Default::default())
}

pub fn send_notification_via_connection_at_bus(
    notification: &Notification,
    id: u32,
    connection: &Connection,
    bus: NotificationBus,
) -> Result<u32> {
    let mut message = build_message("Notify", bus);
    let timeout: i32 = notification.timeout.into();
    message.append_items(&[
        notification.appname.to_owned().into(), // appname
        id.into(),                              // notification to update
        notification.icon.to_owned().into(),    // icon
        notification.summary.to_owned().into(), // summary (title)
        notification.body.to_owned().into(),    // body
        pack_actions(notification),             // actions
        pack_hints(notification)?,              // hints
        timeout.into(),                         // timeout
    ]);

    let reply = connection.send_with_reply_and_block(message, 2000)?;

    match reply.get_items().first() {
        Some(MessageItem::UInt32(ref id)) => Ok(*id),
        _ => Ok(0),
    }
}

pub fn connect_and_send_notification(
    notification: &Notification,
) -> Result<DbusNotificationHandle> {
    let bus = notification.bus.clone();
    connect_and_send_notification_at_bus(notification, bus)
}

pub fn connect_and_send_notification_at_bus(
    notification: &Notification,
    bus: NotificationBus,
) -> Result<DbusNotificationHandle> {
    let connection = Connection::get_private(BusType::Session)?;
    let inner_id = notification.id.unwrap_or(0);
    let id = send_notification_via_connection_at_bus(notification, inner_id, &connection, bus)?;

    Ok(DbusNotificationHandle::new(
        id,
        connection,
        notification.clone(),
    ))
}

fn build_message(method_name: &str, bus: NotificationBus) -> Message {
    Message::new_method_call(
        bus.into_name(),
        NOTIFICATION_OBJECTPATH,
        NOTIFICATION_INTERFACE,
        method_name,
    )
    .unwrap_or_else(|_| panic!("Error building message call {:?}.", method_name))
}

pub fn pack_hints(notification: &Notification) -> Result<MessageItem> {
    if !notification.hints.is_empty() || !notification.hints_unique.is_empty() {
        let hints = notification
            .get_hints()
            .cloned()
            .map(HintMessage::wrap_hint)
            .collect::<Vec<(MessageItem, MessageItem)>>();

        if let Ok(array) = MessageItem::new_dict(hints) {
            return Ok(array);
        }
    }

    Ok(MessageItem::Array(
        MessageItemArray::new(vec![], "a{sv}".into()).unwrap(),
    ))
}

pub fn pack_actions(notification: &Notification) -> MessageItem {
    if !notification.actions.is_empty() {
        let mut actions = vec![];
        for action in &notification.actions {
            actions.push(action.to_owned().into());
        }
        if let Ok(array) = MessageItem::new_array(actions) {
            return array;
        }
    }

    MessageItem::Array(MessageItemArray::new(vec![], "as".into()).unwrap())
}

pub fn get_capabilities() -> Result<Vec<String>> {
    let mut capabilities = vec![];

    let message = build_message("GetCapabilities", Default::default());
    let connection = Connection::get_private(BusType::Session)?;
    let reply = connection.send_with_reply_and_block(message, 2000)?;

    if let Some(MessageItem::Array(items)) = reply.get_items().first() {
        for item in items.iter() {
            if let MessageItem::Str(ref cap) = *item {
                capabilities.push(cap.clone());
            }
        }
    }

    Ok(capabilities)
}

fn unwrap_message_string(item: Option<&MessageItem>) -> String {
    match item {
        Some(MessageItem::Str(value)) => value.to_owned(),
        _ => "".to_owned(),
    }
}

#[allow(clippy::get_first)]
pub fn get_server_information() -> Result<ServerInformation> {
    let message = build_message("GetServerInformation", Default::default());
    let connection = Connection::get_private(BusType::Session)?;
    let reply = connection.send_with_reply_and_block(message, 2000)?;

    let items = reply.get_items();

    Ok(ServerInformation {
        name: unwrap_message_string(items.get(0)),
        vendor: unwrap_message_string(items.get(1)),
        version: unwrap_message_string(items.get(2)),
        spec_version: unwrap_message_string(items.get(3)),
    })
}

/// Listens for the `ActionInvoked(UInt32, String)` Signal.
///
/// No need to use this, check out `Notification::show_and_wait_for_action(FnOnce(action:&str))`
pub fn handle_action(id: u32, func: impl ActionResponseHandler) {
    let connection = Connection::get_private(BusType::Session).unwrap();
    wait_for_action_signal(&connection, id, func);
}

// Listens for the `ActionInvoked(UInt32, String)` signal.
fn wait_for_action_signal(connection: &Connection, id: u32, handler: impl ActionResponseHandler) {
    connection
        .add_match(&format!(
            "interface='{}',member='ActionInvoked'",
            NOTIFICATION_INTERFACE
        ))
        .unwrap();
    connection
        .add_match(&format!(
            "interface='{}',member='NotificationClosed'",
            NOTIFICATION_INTERFACE
        ))
        .unwrap();

    for item in connection.iter(1000) {
        if let ConnectionItem::Signal(message) = item {
            let items = message.get_items();

            let (path, interface, member) = (
                message.path().map_or_else(String::new, |p| {
                    p.into_cstring().to_string_lossy().into_owned()
                }),
                message.interface().map_or_else(String::new, |p| {
                    p.into_cstring().to_string_lossy().into_owned()
                }),
                message.member().map_or_else(String::new, |p| {
                    p.into_cstring().to_string_lossy().into_owned()
                }),
            );
            match (path.as_str(), interface.as_str(), member.as_str()) {
                // match (protocol.unwrap(), iface.unwrap(), member.unwrap()) {
                // Action Invoked
                (path, interface, "ActionInvoked")
                    if path == NOTIFICATION_OBJECTPATH && interface == NOTIFICATION_INTERFACE =>
                {
                    if let (&MessageItem::UInt32(nid), MessageItem::Str(ref action)) =
                        (&items[0], &items[1])
                    {
                        if nid == id {
                            handler.call(&ActionResponse::Custom(action));
                            break;
                        }
                    }
                }

                // Notification Closed
                (path, interface, "NotificationClosed")
                    if path == NOTIFICATION_OBJECTPATH && interface == NOTIFICATION_INTERFACE =>
                {
                    if let (&MessageItem::UInt32(nid), &MessageItem::UInt32(reason)) =
                        (&items[0], &items[1])
                    {
                        if nid == id {
                            handler.call(&ActionResponse::Closed(reason.into()));
                            break;
                        }
                    }
                }
                (..) => (),
            }
        }
    }
}
