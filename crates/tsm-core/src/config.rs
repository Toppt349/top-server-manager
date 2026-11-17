use crate::error::CoreResult;
use crate::util::{read_json_or_default, write_json_atomic};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CloseBehavior {
    /// Hide to tray; servers keep running and the supervisor keeps watching them.
    #[default]
    MinimizeToTray,
    /// Exit the UI but leave servers running (they are re-attached on next launch).
    ExitKeepServers,
    /// Gracefully stop every server, then exit.
    ExitStopServers,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    #[default]
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AppSettings {
    pub language: String,
    pub theme: Theme,
    pub performance_mode: bool,
    pub close_behavior: CloseBehavior,
    pub start_with_windows: bool,
    pub start_minimized: bool,
    /// Allow contacting external services (public IP lookup, endpoint checks).
    pub allow_external_checks: bool,
    /// Always show migration preview even for trusted local manifests.
    pub always_preview_migrations: bool,
    /// Selected version per runtime id (e.g. "php" -> "8.3.14").
    pub runtime_versions: BTreeMap<String, String>,
    pub metrics_interval_secs: u32,
    pub log_buffer_lines: usize,
    pub first_run_done: bool,
    /// Down / recovery notifications (Discord, Telegram).
    pub alerts: AlertSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AlertSettings {
    pub enabled: bool,
    pub discord: bool,
    pub telegram: bool,
    pub telegram_chat_id: String,
    /// Also send a message when a server is back.
    pub notify_recovery: bool,
    /// Watch the public link (tunnel) too, not only the server.
    pub watch_public_link: bool,
    /// A problem must last this long before an alert is sent (no flapping).
    pub delay_secs: u32,
}

impl Default for AlertSettings {
    fn default() -> Self {
        AlertSettings { enabled: false, discord: false, telegram: false, telegram_chat_id: String::new(), notify_recovery: true, watch_public_link: true, delay_secs: 60 }
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        AppSettings {
            language: "th".into(),
            theme: Theme::Dark,
            performance_mode: false,
            close_behavior: CloseBehavior::MinimizeToTray,
            start_with_windows: false,
            start_minimized: false,
            allow_external_checks: false,
            always_preview_migrations: false,
            runtime_versions: BTreeMap::new(),
            metrics_interval_secs: 3,
            log_buffer_lines: 5000,
            first_run_done: false,
            alerts: AlertSettings::default(),
        }
    }
}

impl AppSettings {
    pub fn load(path: &Path) -> CoreResult<Self> {
        read_json_or_default(path)
    }
    pub fn save(&self, path: &Path) -> CoreResult<()> {
        write_json_atomic(path, self)
    }
    pub fn sanitize(&mut self) {
        self.metrics_interval_secs = self.metrics_interval_secs.clamp(1, 60);
        self.log_buffer_lines = self.log_buffer_lines.clamp(500, 50_000);
        self.alerts.delay_secs = self.alerts.delay_secs.clamp(15, 3600);
        self.alerts.telegram_chat_id = self.alerts.telegram_chat_id.trim().chars().take(40).collect();
        if self.language != "th" && self.language != "en" {
            self.language = "en".into();
        }
    }
}
