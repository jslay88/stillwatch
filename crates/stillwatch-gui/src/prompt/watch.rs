//! Status and `StateChanged` for the prompt process. No tray, no session bus
//! unless this process was started without `--bus-address`.

use std::str::FromStr;

use futures_util::SinkExt as _;
use futures_util::StreamExt as _;
use iced::Subscription;
use stillwatch_core::state::State;
use stillwatch_ipc::json::from_json;
use stillwatch_ipc::proxy::StillwatchProxy;
use stillwatch_ipc::status::StatusPayload;
use zbus::proxy::CacheProperties;

/// News from the daemon while the dialog is up.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Wake {
    /// A `Status()` payload. Boxed so the message enum stays small.
    Status(Box<StatusPayload>),
    /// `StateChanged`.
    State(State),
    /// The bus or the daemon couldn't be used.
    Notice(String),
}

/// Follows the daemon on `address` (the session bus when `address` is `None`).
pub(crate) fn subscription(address: Option<&str>) -> Subscription<Wake> {
    let address = address.map(ToOwned::to_owned);
    Subscription::run_with(address, |address| stream(address.as_deref()))
}

fn stream(
    address: Option<&str>,
) -> std::pin::Pin<Box<dyn futures_util::Stream<Item = Wake> + Send>> {
    let address = address.map(ToOwned::to_owned);
    Box::pin(iced::stream::channel(8, async move |mut output| {
        let connection = match crate::bus::connection(address.as_deref()).await {
            Ok(connection) => connection,
            Err(err) => stall(&mut output, err.to_string()).await,
        };
        let proxy = match StillwatchProxy::builder(&connection)
            .cache_properties(CacheProperties::No)
            .build()
            .await
        {
            Ok(proxy) => proxy,
            Err(err) => stall(&mut output, err.to_string()).await,
        };
        if let Ok(json) = proxy.status().await {
            match from_json::<StatusPayload>(&json) {
                Ok(status) => {
                    let _ = output.send(Wake::Status(Box::new(status))).await;
                }
                Err(_) => {
                    tracing::warn!("ignoring a status payload this build doesn't understand");
                }
            }
        }
        let Ok(mut states) = proxy.receive_state_changed().await else {
            std::future::pending::<()>().await;
            return;
        };
        while let Some(signal) = states.next().await {
            let Ok(args) = signal.args() else { continue };
            let Ok(state) = State::from_str(args.state()) else {
                tracing::warn!("ignoring a state signal with an unknown name");
                continue;
            };
            if output.send(Wake::State(state)).await.is_err() {
                break;
            }
        }
    }))
}

/// Tells the window why the bus failed, then waits until the process exits.
async fn stall(output: &mut iced::futures::channel::mpsc::Sender<Wake>, message: String) -> ! {
    let _ = output.send(Wake::Notice(message)).await;
    loop {
        std::future::pending::<()>().await;
    }
}
