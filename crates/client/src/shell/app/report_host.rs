//! The app's part of "Report a bug" (`shell::report`): it knows the match, the saves, the graphics
//! card and the window, so it gathers the facts when the menu button is pressed, makes the report
//! file when the dialog asks, and keeps a snapshot for a crash page.
use super::App;
use crate::shell::module::ModuleUi;
use crate::shell::presentation::motion::Sound;
use crate::shell::report::{self, bundle, GameInfo, ReportInfo};
use crate::shell::saves::tagged;

/// How often (in frames) the snapshot for a crash page is renewed.
const SNAPSHOT_EVERY: u32 = 90;

/// The graphics card, written for the report when the app starts.
pub(super) struct Adapter {
    backend: String,
    name: String,
}

impl Adapter {
    pub(super) fn of(info: &wgpu::AdapterInfo) -> Self {
        let driver = [info.driver.as_str(), info.driver_info.as_str()].iter().filter(|part| !part.is_empty()).copied().collect::<Vec<_>>().join(" ");
        let driver = if driver.is_empty() { String::new() } else { format!(", driver {driver}") }; // i18n-ok: part of the public issue
        Self { backend: format!("{:?}", info.backend), name: format!("{} ({:?}{driver})", info.name, info.device_type) }
    }
}

impl<U: ModuleUi> App<U> {
    /// The facts of this moment.
    pub(super) fn report_info(&mut self) -> ReportInfo {
        let build = report::build();
        let rect = self.ctx.content_rect();
        let scale = self.ctx.pixels_per_point();
        let game = self.module.view().is_some().then(|| GameInfo {
            module: self.module.module_id().name.to_owned(),
            seed: self.module.world_seed(),
            round: self.module.turn(),
            commands: self.module.command_journal().map_or(0, |journal| journal.commands_played()),
        });
        ReportInfo {
            version: build.version.to_owned(),
            git: build.git.map(str::to_owned),
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            backend: self.adapter.backend.clone(),
            adapter: self.adapter.name.clone(),
            agent: report::page().agent,
            page_build: report::page().build,
            language: i18n::language(),
            window: format!("{}x{} px, scale {scale:.2}", (rect.width() * scale).round(), (rect.height() * scale).round()), // i18n-ok: part of the public issue
            game,
            panic: report::last_panic(),
            trace: report::last_trace(),
            log: report::log_tail(),
        }
    }

    /// The menu's "Report a bug": gather the facts and open the dialog.
    pub(super) fn open_report(&mut self) {
        let info = self.report_info();
        let prepared = report::prepare(&info);
        self.screens.report.show(info, prepared);
        self.sounds.push(Sound::Select);
        self.ctx.request_repaint();
    }

    /// The dialog's "Save report file": the facts as shown, the match as it is, and on a desktop the
    /// autosave with the commands since it.
    pub(super) fn save_report(&mut self) {
        let Some((header, name)) = self.screens.report.prepared().map(|(_, prepared)| (prepared.header.clone(), prepared.file_name.clone())) else {
            return;
        };
        let module = self.module.module_id();
        let mut parts = bundle::Parts { current_save: self.module.save_bytes().map(|bytes| tagged(module, &bytes)), ..Default::default() };
        if let Some(journal) = &self.journal {
            parts.autosave = crate::shell::storage::read(&journal.path().with_extension("aurelia")).ok();
            parts.journal = crate::shell::storage::read(journal.path()).ok();
        }
        let built = bundle::build(&header, &parts, report::unix_seconds());
        let result = report::deliver(&name, &built.bytes);
        if result.is_ok() {
            self.sounds.push(Sound::Notification);
        }
        self.screens.report.saved(result);
        self.ctx.request_repaint();
    }

    /// For a launcher that shows a crash page: renew the snapshot it will show from.
    pub(super) fn remember_report(&mut self) {
        self.report_tick = self.report_tick.wrapping_add(1);
        if !report::wants_snapshots() || self.report_tick % SNAPSHOT_EVERY != 1 {
            return;
        }
        let mut info = self.report_info();
        info.panic = None;
        info.trace = None;
        info.log.clear();
        report::remember(info, report::dialog_words());
    }
}
