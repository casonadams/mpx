//! Config, options and key bindings: `reload-config`, `set-option`, `show-options`, `bind-key`.
use crate::config::Config;
use crate::tui::app::{App, Flow};
use crate::tui::keymap;
use crate::tui::overlay::Overlay;

impl App {
    /// Re-reads the config file. A file that fails to parse, or a keymap that fails to resolve,
    /// leaves everything unchanged. The daemon reloads the settings it owns (resume rules,
    /// restore, history limit and spawn defaults) from the same file.
    pub(super) async fn reload_config(&mut self) -> Flow {
        let path = self.launch.config_path.clone().or_else(Config::path);
        let config = match Config::load_at(path.as_deref()) {
            Ok(config) => config,
            Err(e) => return self.say(format!("reload-config: {e}")),
        };
        if let Err(e) = keymap::resolve(&config) {
            return self.say(format!("reload-config: {e}"));
        }
        let daemon = self.client.reload_config(path.as_deref()).await;
        self.config = config;
        self.apply_bindings().await;
        self.layout.dirty = true;
        match daemon {
            Ok(()) => self.say("config reloaded"),
            Err(e) => self.say(format!("config reloaded in the UI only: {e}")),
        }
    }

    /// `set-option`: changes this client's option. A value that does not parse, or a prefix the
    /// keymap cannot resolve, leaves the config unchanged.
    pub(super) async fn set_option(
        &mut self,
        name: &str,
        value: Option<&str>,
        unset: bool,
        quiet: bool,
    ) -> Flow {
        if quiet && !crate::options::is_known(name) {
            return Flow::Continue;
        }
        let mut config = self.config.clone();
        let result = match value {
            Some(value) if !unset => crate::options::set(&mut config, name, value),
            _ => crate::options::unset(&mut config, name),
        };
        if let Err(e) = result.and_then(|()| keymap::resolve(&config).map(|_| ())) {
            return self.say(format!("set-option: {e}"));
        }
        self.config = config;
        self.apply_bindings().await;
        Flow::Continue
    }

    /// `show-options`: every option in an overlay, or one on the status row.
    pub(super) async fn show_options(
        &mut self,
        name: Option<&str>,
        value_only: bool,
        quiet: bool,
    ) -> Flow {
        let synchronize = self
            .layout
            .tabs
            .iter()
            .any(|t| t.is_active && t.synchronized);
        let synchronize = if synchronize { "on" } else { "off" };
        if name == Some("synchronize-panes") {
            return if value_only {
                self.say(synchronize)
            } else {
                self.say(format!("synchronize-panes {synchronize}"))
            };
        }
        let Some(name) = name else {
            let mut options = crate::options::list(&self.config);
            options.push(("synchronize-panes", synchronize.to_string()));
            options.sort_by_key(|(name, _)| *name);
            let lines = options
                .into_iter()
                .map(|(name, value)| format!("{name} {value}"))
                .collect();
            return self.open_overlay(Overlay::text("options", lines)).await;
        };
        match crate::options::get(&self.config, name) {
            Ok(value) if value_only => self.say(value),
            Ok(value) => self.say(format!("{name} {value}")),
            Err(_) if quiet && !crate::options::is_known(name) => Flow::Continue,
            Err(e) => self.say(format!("show-options: {e}")),
        }
    }

    /// Rebuilds the keymap: the config's bindings, then the session's `bind-key` / `unbind-key`
    /// changes. A binding that does not apply is reported and skipped.
    pub async fn apply_bindings(&mut self) {
        let mut problems = Vec::new();
        let mut keymap = match keymap::resolve(&self.config) {
            Ok(keymap) => keymap,
            Err(e) => {
                problems.push(e);
                self.keymap.clone()
            }
        };
        for (name, line) in self.client.get_bindings().await.unwrap_or_default() {
            let (table, key) = name.split_once(':').unwrap_or(("prefix", &name));
            let table = keymap::Table::from_name(table).unwrap_or(keymap::Table::Prefix);
            if let Err(e) = keymap.bind(table, key, &line) {
                problems.push(e);
            }
        }
        self.keymap = keymap;
        self.layout.dirty = true;
        if let Some(first) = problems.first() {
            self.say(first.clone());
        }
    }

    /// `bind-key` / `unbind-key`: changes this keymap and records the binding in the session, so
    /// terminals attaching later have it too.
    pub(super) async fn bind_key(&mut self, table: keymap::Table, key: &str, line: &str) -> Flow {
        if let Err(e) = self.keymap.bind(table, key, line) {
            return self.say(e);
        }
        let name = format!("{}:{key}", table.name());
        let result = self.client.set_binding(&name, Some(line)).await;
        self.settle(result)
    }
}
