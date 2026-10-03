//! ksni adapter. The menu contents come from [`crate::tray::tray_model`];
//! this file only turns them into a `StatusNotifierItem`.

use ksni::TrayMethods as _;
use ksni::menu::StandardItem;
use tokio::sync::mpsc;

use crate::icons;
use crate::shell::TrayAction;
use crate::tray::{MenuEntry, TrayModel};

/// The status item Plasma shows.
pub struct GuiTray {
    model: TrayModel,
    actions: mpsc::Sender<TrayAction>,
}

impl GuiTray {
    /// Replaces the icon, tooltip, and menu.
    pub fn set_model(&mut self, model: TrayModel) {
        self.model = model;
    }

    fn item(&self, entry: &MenuEntry) -> ksni::MenuItem<Self> {
        let action = entry.action;
        let actions = self.actions.clone();
        StandardItem {
            label: entry.label.clone(),
            activate: Box::new(move |_| {
                if let Err(err) = actions.try_send(action) {
                    tracing::warn!("dropped tray action: {err}");
                }
            }),
            ..StandardItem::default()
        }
        .into()
    }
}

impl ksni::Tray for GuiTray {
    fn id(&self) -> String {
        "stillwatch".to_owned()
    }

    fn title(&self) -> String {
        "Stillwatch".to_owned()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        vec![icons::pixmap(self.model.icon)]
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: self.model.title.clone(),
            description: self.model.description.clone(),
            icon_pixmap: vec![icons::pixmap(self.model.icon)],
            ..ksni::ToolTip::default()
        }
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        self.model
            .entries
            .iter()
            .map(|entry| self.item(entry))
            .collect()
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        if let Err(err) = self.actions.try_send(TrayAction::OpenSettings) {
            tracing::warn!("dropped tray click: {err}");
        }
    }

    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        tracing::warn!("status notifier watcher is offline: {reason:?}");
        true
    }
}

/// Registers the tray on the session bus.
///
/// Assumes a `StatusNotifierWatcher` will show up, so starting before Plasma's
/// panel is ready doesn't fail the process.
///
/// # Errors
///
/// Returns the ksni error when the tray service can't be created at all.
pub async fn spawn(
    model: TrayModel,
    actions: mpsc::Sender<TrayAction>,
) -> Result<ksni::Handle<GuiTray>, ksni::Error> {
    GuiTray { model, actions }
        .assume_sni_available(true)
        .spawn()
        .await
}
